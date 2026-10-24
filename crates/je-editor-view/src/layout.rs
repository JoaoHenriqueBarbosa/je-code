//! A geometria da view: o `EditorLayoutInfoComputer` do Monaco (margem de glifos, números de linha,
//! decorações) e as conversões entre posição do modelo e pixel.
//!
//! Medidas conferidas contra o oráculo (VS Code 1.143.1, fonte 14 px, `lineHeight` 19): margem de glifos
//! 19 px, números de linha `round(max(dígitos, 3) * larguraDoDígito)` (25 px até 999 linhas), decorações
//! 26 px (10 de `lineDecorationsWidth` e 16 do controle de dobra), conteúdo começando 70 px à direita da margem.

use je_editor::cursor_columns::next_render_tab_stop;
use je_text::{TextStyle, with_text_system};

/// A lista de famílias padrão do editor: o padrão de `editor.fontFamily` de cada sistema (no Linux, a lista que o
/// oráculo resolve, `"Droid Sans Mono", "monospace", monospace`).
#[cfg(target_os = "windows")]
pub const DEFAULT_FONT_FAMILY: &str = "Consolas, \"Courier New\", monospace";
#[cfg(target_os = "macos")]
pub const DEFAULT_FONT_FAMILY: &str = "Menlo, Monaco, \"Courier New\", monospace";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const DEFAULT_FONT_FAMILY: &str = "\"Droid Sans Mono\", \"monospace\", monospace";
/// `editor.fontSize` padrão (`EDITOR_FONT_DEFAULTS.fontSize`: 12 no macOS, 14 nos outros).
pub const DEFAULT_FONT_SIZE: f32 = if cfg!(target_os = "macos") { 12.0 } else { 14.0 };
/// A altura de linha que `editor.lineHeight: 0` dá com a fonte padrão (`GOLDEN_LINE_HEIGHT_RATIO` vezes o tamanho).
pub const DEFAULT_LINE_HEIGHT: f64 = if cfg!(target_os = "macos") { 18.0 } else { 19.0 };
/// `lineNumbersMinChars` do editor de texto do workbench.
pub const LINE_NUMBERS_MIN_CHARS: usize = 3;
/// `lineDecorationsWidth` (10) mais o controle de dobra (16).
pub const LINE_DECORATIONS_WIDTH: f64 = 26.0;

/// As medidas da fonte do editor (o `FontInfo`).
#[derive(Clone, Debug, PartialEq)]
pub struct FontMetrics {
    pub families: String,
    pub size: f32,
    /// `editor.fontWeight` do texto normal (o negrito dos tokens é sempre 700).
    pub weight: u16,
    pub line_height: f64,
    /// A largura de um caractere, medida como o `CharWidthReader` do Monaco (256 repetições divididas por 256).
    pub char_width: f64,
    /// A maior largura de dígito, medida do mesmo jeito.
    pub digit_width: f64,
}

impl FontMetrics {
    pub fn measure(families: &str, size: f32, line_height: f64) -> Self {
        Self::measure_weighted(families, size, 400, line_height)
    }

    /// Como [`FontMetrics::measure`], com o peso da fonte (`editor.fontWeight`).
    pub fn measure_weighted(families: &str, size: f32, weight: u16, line_height: f64) -> Self {
        let style = TextStyle::new(families, size, weight);
        let width_of = |ch: char| {
            let text: String = std::iter::repeat_n(ch, 256).collect();
            f64::from(with_text_system(|t| t.line(&text, &style)).width) / 256.0
        };
        let char_width = width_of('a');
        let digit_width = ('0'..='9').map(width_of).fold(0.0, f64::max);
        Self { families: families.to_owned(), size, weight, line_height, char_width, digit_width }
    }

    pub fn style(&self, scale: f64, bold: bool, italic: bool) -> TextStyle {
        TextStyle::new(self.families.as_str(), (f64::from(self.size) * scale) as f32, if bold { 700 } else { self.weight }).italic(italic)
    }
}

/// Um espaço vertical entre linhas (o `IViewZone` do Monaco): `height` px depois da linha `after_line` (0 é antes da
/// primeira), em linhas da view. O editor de diff usa para alinhar as linhas dos dois lados.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewZone {
    pub after_line: i32,
    pub height: f64,
}

/// A geometria da view para um tamanho e uma contagem de linhas.
#[derive(Clone, Debug)]
pub struct ViewLayout {
    pub metrics: FontMetrics,
    pub tab_size: i32,
    pub width: f64,
    pub height: f64,
    /// As linhas da view (com quebra de linha e dobra, as do `ViewModel`).
    pub line_count: i32,
    /// As linhas do modelo, de onde sai a largura dos números de linha (`lineNumbersDigitCount`).
    pub model_line_count: i32,
    /// `editor.lineNumbers` diferente de `off`: a margem reserva a coluna dos números.
    pub show_line_numbers: bool,
    /// `editor.minimap.enabled`.
    pub minimap_enabled: bool,
    /// A coluna das decorações de linha: 10 px do `lineDecorationsWidth` mais 16 do controle de dobra; no editor de
    /// diff (sem dobra) são 16 px (medido no oráculo).
    pub decorations_width: f64,
    /// Os espaços entre linhas, em ordem de `after_line` (cada um com o deslocamento acumulado até ele).
    zones: Vec<(ViewZone, f64)>,
}

impl ViewLayout {
    pub fn new(metrics: FontMetrics, tab_size: i32, width: f64, height: f64, line_count: i32) -> Self {
        Self { metrics, tab_size, width, height, line_count, model_line_count: line_count, show_line_numbers: true, minimap_enabled: true, decorations_width: LINE_DECORATIONS_WIDTH, zones: Vec::new() }
    }

    /// Troca os espaços entre linhas (ordena por posição; os de mesma linha somam).
    pub fn set_zones(&mut self, mut zones: Vec<ViewZone>) {
        zones.sort_by_key(|z| z.after_line);
        let mut total = 0.0;
        self.zones = zones
            .into_iter()
            .map(|z| {
                total += z.height;
                (z, total)
            })
            .collect();
    }

    pub fn zones(&self) -> impl Iterator<Item = ViewZone> + '_ {
        self.zones.iter().map(|(z, _)| *z)
    }

    /// A altura de todos os espaços.
    pub fn zones_height(&self) -> f64 {
        self.zones.last().map_or(0.0, |(_, total)| *total)
    }

    /// A altura dos espaços que ficam antes da linha `line_number` (depois das linhas anteriores a ela).
    fn zones_before_line(&self, line_number: i32) -> f64 {
        let count = self.zones.partition_point(|(z, _)| z.after_line < line_number);
        count.checked_sub(1).map_or(0.0, |i| self.zones[i].1)
    }

    /// O topo da linha no documento, com os espaços (sem a rolagem).
    pub fn line_offset(&self, line_number: i32) -> f64 {
        f64::from(line_number - 1) * self.line_height() + self.zones_before_line(line_number)
    }

    /// O topo e a altura de cada espaço no documento (sem a rolagem).
    pub fn zone_spans(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.zones.iter().map(|(z, cumulative)| (f64::from(z.after_line) * self.line_height() + cumulative - z.height, z.height))
    }

    pub fn glyph_margin_width(&self) -> f64 {
        self.metrics.line_height
    }

    /// `Math.round(max(dígitos, minChars) * maxDigitWidth)`.
    pub fn line_numbers_width(&self) -> f64 {
        if !self.show_line_numbers {
            return 0.0;
        }
        let digits = self.model_line_count.max(1).to_string().len().max(LINE_NUMBERS_MIN_CHARS);
        (digits as f64 * self.metrics.digit_width).round()
    }

    /// A borda esquerda dos números de linha.
    pub fn line_numbers_left(&self) -> f64 {
        self.glyph_margin_width()
    }

    /// O início do conteúdo (`contentLeft`).
    pub fn content_left(&self) -> f64 {
        self.glyph_margin_width() + self.line_numbers_width() + self.decorations_width
    }

    /// A largura da barra de rolagem vertical (`verticalScrollbarSize`).
    pub const SCROLLBAR_WIDTH: f64 = 14.0;
    /// A largura de cada coluna do minimapa em px (`minimap.scale` 1 e `renderCharacters`).
    pub const MINIMAP_CHAR_WIDTH: f64 = 1.0;
    /// A altura de cada linha do minimapa.
    pub const MINIMAP_LINE_HEIGHT: f64 = 2.0;
    /// A margem à esquerda do texto do minimapa (`MINIMAP_GUTTER_WIDTH`).
    pub const MINIMAP_GUTTER: f64 = 8.0;

    /// A largura do minimapa (`computeMinimapLayout` no modo `proportional`, com `maxColumn` 120): a fração da
    /// largura externa que deixa cada coluna do minimapa com a largura de um caractere típico do editor.
    pub fn minimap_width(&self) -> f64 {
        if !self.minimap_enabled {
            return 0.0;
        }
        let outer = self.width - Self::SCROLLBAR_WIDTH;
        let fraction = (outer * Self::MINIMAP_CHAR_WIDTH / (self.metrics.char_width + Self::MINIMAP_CHAR_WIDTH)).floor();
        // `maxColumn` 120 mais a margem do minimapa.
        fraction.clamp(0.0, 120.0 * Self::MINIMAP_CHAR_WIDTH + Self::MINIMAP_GUTTER)
    }

    /// A borda direita da área do texto (onde começa o minimapa).
    pub fn text_right(&self) -> f64 {
        (self.width - Self::SCROLLBAR_WIDTH - self.minimap_width()).max(self.content_left())
    }

    pub fn content_width(&self) -> f64 {
        (self.text_right() - self.content_left()).max(0.0)
    }

    pub fn line_height(&self) -> f64 {
        self.metrics.line_height
    }

    /// Altura total das linhas mais o espaço de `scrollBeyondLastLine` (a última linha pode subir até o topo).
    pub fn scroll_height(&self) -> f64 {
        f64::from(self.line_count) * self.line_height() + self.zones_height() + (self.height - self.line_height()).max(0.0)
    }

    pub fn max_scroll_top(&self) -> f64 {
        (self.scroll_height() - self.height).max(0.0)
    }

    /// O topo da linha na tela (relativo à view) para um `scroll_top`.
    pub fn line_top(&self, line_number: i32, scroll_top: f64) -> f64 {
        self.line_offset(line_number) - scroll_top
    }

    /// A linha sob a coordenada `y` (relativa à view), limitada ao documento.
    pub fn line_at_y(&self, y: f64, scroll_top: f64) -> i32 {
        let document_y = y + scroll_top;
        if self.zones.is_empty() {
            let line = (document_y / self.line_height()).floor() as i64 + 1;
            return line.clamp(1, i64::from(self.line_count)) as i32;
        }
        // Uma busca binária pela última linha cujo topo não passa de `document_y` (um ponto dentro de um espaço
        // pertence à linha de cima).
        let (mut low, mut high) = (1, self.line_count.max(1));
        while low < high {
            let mid = low + (high - low + 1) / 2;
            if self.line_offset(mid) <= document_y {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        low
    }

    /// As linhas parcialmente visíveis, de `(primeira, última)`.
    pub fn visible_lines(&self, scroll_top: f64) -> (i32, i32) {
        let first = self.line_at_y(0.0, scroll_top);
        let last = self.line_at_y(self.height - 0.001, scroll_top);
        (first, last)
    }

    /// A coluna visual (em células, com tabulação) de uma coluna do modelo, para linhas sem caractere largo.
    pub fn visual_column(&self, line: &[u16], column: i32) -> i32 {
        je_editor::cursor_columns::visible_column_from_column(line, column, self.tab_size)
    }

    /// Posição horizontal (relativa ao início do conteúdo) de uma coluna visual, na grade monoespaçada.
    pub fn x_of_visual_column(&self, visual_column: i32) -> f64 {
        f64::from(visual_column) * self.metrics.char_width
    }

    /// A coluna visual mais próxima de um `x` relativo ao início do conteúdo (arredonda para a borda mais próxima).
    pub fn visual_column_at_x(&self, x: f64) -> i32 {
        (x / self.metrics.char_width).round().max(0.0) as i32
    }

    /// A largura de uma tabulação que começa na coluna visual dada, em células.
    pub fn tab_cells(&self, visual_column: i32) -> i32 {
        next_render_tab_stop(visual_column, self.tab_size) - visual_column
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(lines: i32) -> ViewLayout {
        let metrics = FontMetrics { families: String::new(), size: 14.0, weight: 400, line_height: 19.0, char_width: 8.4296875, digit_width: 8.4296875 };
        ViewLayout::new(metrics, 4, 1000.0, 400.0, lines)
    }

    #[test]
    fn content_left_matches_the_oracle() {
        // Margem de glifos 19, números 25, decorações 26: o conteúdo começa 70 px à direita.
        assert_eq!(layout(29).line_numbers_width(), 25.0);
        assert_eq!(layout(29).content_left(), 70.0);
        // Com 5 dígitos a coluna de números cresce.
        assert_eq!(layout(12_345).line_numbers_width(), 42.0);
    }

    #[test]
    fn line_to_pixel_and_back() {
        let l = layout(100);
        for line in [1, 2, 50, 100] {
            let top = l.line_top(line, 95.0);
            assert_eq!(l.line_at_y(top, 95.0), line);
            assert_eq!(l.line_at_y(top + 18.9, 95.0), line);
            assert_eq!(l.line_at_y(top + 19.0, 95.0), (line + 1).min(100));
        }
        assert_eq!(l.line_at_y(-500.0, 0.0), 1);
        assert_eq!(l.line_at_y(1e9, 0.0), 100);
    }

    #[test]
    fn view_zones_push_the_lines_below_them_down() {
        let mut l = layout(100);
        // Um espaço de 2 linhas depois da linha 3 e outro de 1 linha antes da primeira.
        l.set_zones(vec![ViewZone { after_line: 3, height: 38.0 }, ViewZone { after_line: 0, height: 19.0 }]);
        assert_eq!(l.zones_height(), 57.0);
        assert_eq!(l.line_top(1, 0.0), 19.0, "o espaço do topo empurra a linha 1");
        assert_eq!(l.line_top(3, 0.0), 19.0 + 2.0 * 19.0);
        assert_eq!(l.line_top(4, 0.0), 19.0 + 3.0 * 19.0 + 38.0, "a linha 4 vem depois do espaço");
        assert_eq!(l.scroll_height(), 100.0 * 19.0 + 57.0 + (400.0 - 19.0));
        let spans: Vec<(f64, f64)> = l.zone_spans().collect();
        assert_eq!(spans, vec![(0.0, 19.0), (19.0 + 3.0 * 19.0, 38.0)]);
        // Os pontos de cada linha voltam a ela; um ponto dentro de um espaço pertence à linha de cima.
        for line in [1, 2, 3, 4, 50, 100] {
            let top = l.line_top(line, 0.0);
            assert_eq!(l.line_at_y(top, 0.0), line);
            assert_eq!(l.line_at_y(top + 18.9, 0.0), line);
        }
        assert_eq!(l.line_at_y(l.line_top(3, 0.0) + 19.0 + 10.0, 0.0), 3);
        assert_eq!(l.line_at_y(0.0, 0.0), 1, "dentro do espaço do topo");
        assert_eq!(l.visible_lines(0.0).0, 1);
    }

    #[test]
    fn visual_column_to_x_and_back() {
        let l = layout(10);
        for v in 0..80 {
            assert_eq!(l.visual_column_at_x(l.x_of_visual_column(v)), v);
            // O meio da célula arredonda para a borda mais próxima.
            assert_eq!(l.visual_column_at_x(l.x_of_visual_column(v) + 4.0), v);
            assert_eq!(l.visual_column_at_x(l.x_of_visual_column(v) + 4.5), v + 1);
        }
        assert_eq!(l.visual_column_at_x(-30.0), 0);
    }

    #[test]
    fn visible_lines_and_max_scroll() {
        let l = layout(1000);
        assert_eq!(l.visible_lines(0.0), (1, 22));
        assert_eq!(l.visible_lines(190.0), (11, 32));
        // `scrollBeyondLastLine`: a última linha chega ao topo.
        assert_eq!(l.max_scroll_top(), 1000.0 * 19.0 - 19.0);
    }

    #[test]
    fn tab_fills_to_next_stop() {
        let l = layout(1);
        assert_eq!(l.tab_cells(0), 4);
        assert_eq!(l.tab_cells(1), 3);
        assert_eq!(l.tab_cells(4), 4);
    }
}

#[cfg(test)]
mod minimap_tests {
    use super::*;

    /// Medidas do oráculo (VS Code 1.143.1, editor de 842 x 677 px, 29 linhas, fonte de 14 px): minimapa de 88 px e
    /// botão da barra com 379 px.
    #[test]
    fn minimap_and_scrollbar_match_the_oracle() {
        // O caractere típico do oráculo (Droid Sans Mono 14 px): 8,4 px; a fonte medida aqui depende da máquina.
        let metrics = FontMetrics { char_width: 8.4, ..FontMetrics::measure(DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE, DEFAULT_LINE_HEIGHT) };
        let layout = ViewLayout::new(metrics, 4, 842.0, 677.0, 29);
        assert_eq!(layout.minimap_width(), 88.0);
        assert_eq!(layout.text_right(), 842.0 - 14.0 - 88.0);
        let size = (layout.height * layout.height / layout.scroll_height()).floor();
        assert_eq!(size, 379.0);
    }
}
