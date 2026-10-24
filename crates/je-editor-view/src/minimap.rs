//! O minimapa e a barra de rolagem vertical do editor (`viewParts/minimap/minimap.ts` e o `ScrollableElement`),
//! com as medidas lidas do oráculo: minimapa de 1 px por coluna e 2 px por linha com os caracteres como pontos
//! na cor do token (`renderCharacters`, `scale` 1), botão da barra de 14 px com as cores `scrollbarSlider.*`.
//!
//! O minimapa de um documento maior que a altura rola junto com o editor, em proporção: o topo do documento no
//! topo do minimapa quando a rolagem é zero e o fim no fim quando ela é máxima.

use je_editor::cursor_common::CursorSimpleModel;
use je_editor::line_tokens::IViewLineTokens;
use je_render::kurbo::Rect;
use je_render::{DisplayList, Rgba, Shape};

use crate::view::EditorView;

/// A opacidade do ponto de cada caractere nas duas linhas de pixels de uma linha do minimapa, medida no oráculo
/// (a média dos pixels de `keyword` sobre o fundo: o glifo do minimapa é uma amostra de 5x7 reduzida a 1x2).
const GLYPH_ALPHA: [u8; 2] = [90, 68];

impl EditorView {
    /// O deslocamento vertical do conteúdo do minimapa em px.
    fn minimap_offset(&self) -> f64 {
        let total = f64::from(self.layout.line_count) * crate::layout::ViewLayout::MINIMAP_LINE_HEIGHT;
        let overflow = (total - self.layout.height).max(0.0);
        let max = self.layout.max_scroll_top();
        if max <= 0.0 { 0.0 } else { (self.scroll_top() / max).clamp(0.0, 1.0) * overflow }
    }

    /// A linha do documento sob a coordenada `y` do minimapa (relativa à view), limitada ao documento.
    pub(crate) fn minimap_line_at(&self, y: f64) -> i32 {
        let line = ((y + self.minimap_offset()) / crate::layout::ViewLayout::MINIMAP_LINE_HEIGHT).floor() as i64 + 1;
        line.clamp(1, i64::from(self.layout.line_count)) as i32
    }

    /// Rola para centrar a linha `line` (o clique no minimapa).
    pub(crate) fn scroll_to_center_line(&mut self, line: i32) {
        let top = f64::from(line - 1) * self.layout.line_height() - (self.layout.height - self.layout.line_height()) / 2.0;
        self.set_scroll_top(top);
    }

    /// O botão da barra vertical: topo e altura em px da view.
    pub(crate) fn scrollbar_slider(&self) -> (f64, f64) {
        let (height, scroll_height) = (self.layout.height, self.layout.scroll_height());
        let size = (height * height / scroll_height).floor().clamp(20.0_f64.min(height), height);
        let max = self.layout.max_scroll_top();
        let top = if max <= 0.0 { 0.0 } else { (self.scroll_top() / max).clamp(0.0, 1.0) * (height - size) };
        (top.round(), size)
    }

    /// Rola para o botão da barra ficar com o topo em `slider_top`.
    pub(crate) fn scroll_to_slider_top(&mut self, slider_top: f64) {
        let (_, size) = self.scrollbar_slider();
        let travel = self.layout.height - size;
        if travel > 0.0 {
            self.set_scroll_top(slider_top / travel * self.layout.max_scroll_top());
        }
    }

    pub(crate) fn paint_minimap(&mut self, list: &mut DisplayList, origin: (f64, f64), scale: f64) {
        use crate::layout::ViewLayout as L;
        let area_left = self.layout.text_right();
        let width = self.layout.minimap_width();
        if width < L::MINIMAP_GUTTER + 1.0 {
            return;
        }
        let height = self.layout.height;
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| Rect::new(((origin.0 + x0) * scale).round(), ((origin.1 + y0) * scale).round(), ((origin.0 + x1) * scale).round(), ((origin.1 + y1) * scale).round());
        list.push_clip(Shape::Rect(rect(area_left, 0.0, area_left + width, height)));
        list.fill_rect(rect(area_left, 0.0, area_left + width, height), self.colors.minimap_background);
        let offset = self.minimap_offset();
        let first = ((offset / L::MINIMAP_LINE_HEIGHT).floor() as i32 + 1).max(1);
        let last = (((offset + height) / L::MINIMAP_LINE_HEIGHT).ceil() as i32).min(self.layout.line_count);
        let columns = (width - L::MINIMAP_GUTTER).max(0.0) as usize;
        let tab = self.layout.tab_size.max(1) as usize;
        for line_number in first..=last {
            let top = f64::from(line_number - 1) * L::MINIMAP_LINE_HEIGHT - offset;
            let content = self.model.get_line_content(line_number);
            let units = content.units();
            if units.iter().all(|u| *u == 0x20 || *u == 0x09) {
                continue;
            }
            let tokens = self.model.tokenization().get_line_tokens(line_number);
            // A coluna de cada unidade (a tabulação avança até o próximo múltiplo de `tab`).
            let mut column = 0usize;
            let mut token_start = 0usize;
            'tokens: for index in 0..tokens.get_count() {
                let token_end = (tokens.get_end_offset(index) as usize).min(units.len());
                let color = self.token_color(tokens.get_metadata(index));
                let mut run_start: Option<usize> = None;
                for &unit in &units[token_start..token_end] {
                    let blank = unit == 0x20 || unit == 0x09;
                    if column >= columns {
                        break 'tokens;
                    }
                    if !blank && run_start.is_none() {
                        run_start = Some(column);
                    }
                    let next = if unit == 0x09 { (column / tab + 1) * tab } else { column + 1 };
                    if blank && let Some(start) = run_start.take() {
                        self.minimap_run(list, &rect, area_left, top, start, column, color);
                    }
                    column = next;
                }
                if let Some(start) = run_start {
                    self.minimap_run(list, &rect, area_left, top, start, column.min(columns), color);
                }
                token_start = token_end;
            }
        }
        list.pop_clip();
    }

    #[allow(clippy::too_many_arguments)]
    fn minimap_run(&self, list: &mut DisplayList, rect: &dyn Fn(f64, f64, f64, f64) -> Rect, area_left: f64, top: f64, from: usize, to: usize, color: Rgba) {
        use crate::layout::ViewLayout as L;
        let x0 = area_left + L::MINIMAP_GUTTER + from as f64 * L::MINIMAP_CHAR_WIDTH;
        let x1 = area_left + L::MINIMAP_GUTTER + to as f64 * L::MINIMAP_CHAR_WIDTH;
        for (row, alpha) in GLYPH_ALPHA.iter().enumerate() {
            let y = top + row as f64 * L::MINIMAP_LINE_HEIGHT / 2.0;
            list.fill_rect(rect(x0, y, x1, y + L::MINIMAP_LINE_HEIGHT / 2.0), Rgba { a: ((u16::from(color.a) * u16::from(*alpha)) / 255) as u8, ..color });
        }
    }

    pub(crate) fn paint_scrollbar(&mut self, list: &mut DisplayList, origin: (f64, f64), scale: f64) {
        let (top, size) = self.scrollbar_slider();
        if self.layout.max_scroll_top() <= 0.0 {
            return;
        }
        let left = self.layout.width - crate::layout::ViewLayout::SCROLLBAR_WIDTH;
        let color = match self.drag {
            Some(crate::view::DragMode::Scrollbar(_)) => self.colors.scrollbar_slider_active,
            _ => self.colors.scrollbar_slider,
        };
        let device = |x: f64, y: f64| (((origin.0 + x) * scale).round(), ((origin.1 + y) * scale).round());
        let (x0, y0) = device(left, top);
        let (x1, y1) = device(self.layout.width, top + size);
        list.fill_rect(Rect::new(x0, y0, x1, y1), color);
    }
}
