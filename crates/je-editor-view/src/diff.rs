//! O editor de diff lado a lado (`DiffEditorWidget` do Monaco, `vs/editor/browser/widget/diffEditor`): o original à
//! esquerda (somente leitura), o modificado à direita, a calha de 35 px entre eles e a régua de visão geral de
//! 30 px à direita.
//!
//! As medidas vêm do DOM do oráculo (VS Code 1.143.1): a largura do conteúdo é a da janela menos os 30 px da régua,
//! o sash fica em `floor(0.5 * conteúdo)`, o original ocupa o sash menos a calha e o modificado o resto, com 1 px de
//! `diffEditor.border` na borda esquerda. O alinhamento é o de `computeRangeAlignment`: cada trecho que muda ganha, do
//! lado mais curto, um espaço (a zona de visão) hachurado com `diffEditor.diagonalFill`, e as linhas e os trechos
//! alterados levam os fundos de `diffEditorDecorations.ts`. A rolagem dos dois lados anda junta.
//!
//! O modificado é a [`EditorView`] de sempre (o shell a mantém no `OpenEditor`, com salvar, busca e o resto); o
//! [`DiffEditor`] guarda só o original e o estado do diff e recebe o modificado em cada chamada.

use je_editor::cursor_common::CursorSimpleModel;
use je_editor::diff::default_lines_diff_computer::default_lines_diff_computer::DefaultLinesDiffComputer;
use je_editor::diff::lines_diff_computer::{ILinesDiffComputer, LinesDiffComputerOptions};
use je_editor::diff::range_mapping::DetailedLineRangeMapping;
use je_editor::text_model::TextModel;
use je_render::kurbo::Rect;
use je_render::{DisplayList, Rgba, Shape};
use je_theme::{ColorTheme, ThemeType};
use je_ui::{Modifiers, PointerButton};
use std::rc::Rc;
use web_time::Instant;

use crate::colors::EditorColors;
use crate::input::EditorKey;
use crate::layout::ViewZone;
use crate::view::{EditorView, LineDecoration, LineSign, RangeDecoration};

/// A largura da calha entre os dois editores (`DiffEditorGutter.width`).
pub const GUTTER_WIDTH: f64 = 35.0;
/// A largura da régua de visão geral (`ENTIRE_DIFF_OVERVIEW_WIDTH`): 15 px do original e 15 px do modificado.
pub const OVERVIEW_WIDTH: f64 = 30.0;
/// A altura mínima de um bloco na régua de visão geral (`OverviewZoneManager.MINIMUM_HEIGHT`).
const OVERVIEW_MINIMUM_HEIGHT: f64 = 4.0;
/// O prazo do cálculo do diff (`diffEditor.maxComputationTime`, em ms).
const MAX_COMPUTATION_TIME_MS: f64 = 5000.0;

/// O lado de um editor do diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Original,
    Modified,
}

/// As cores do editor de diff resolvidas do tema.
#[derive(Clone, Debug)]
pub struct DiffColors {
    pub inserted_line: Rgba,
    pub removed_line: Rgba,
    pub inserted_text: Rgba,
    pub removed_text: Rgba,
    pub diagonal_fill: Rgba,
    pub border: Rgba,
    pub overview_inserted: Rgba,
    pub overview_removed: Rgba,
    /// O fundo da régua (`.diffOverview`: branco a 1% no escuro, preto a 3% no claro).
    pub overview_background: Rgba,
    /// O botão da régua (`.diffViewport`): as cores da barra de rolagem.
    pub viewport: Rgba,
}

impl DiffColors {
    pub fn from_theme(theme: &ColorTheme) -> Self {
        let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0x00, 0xff));
        let inserted_text = color("diffEditor.insertedTextBackground");
        let removed_text = color("diffEditor.removedTextBackground");
        // `theme.getColor(diffInsertedLine) || diffInserted`, como o CSS do original.
        let line = |id: &str, fallback: Rgba| theme.get_color(id).filter(|c| c.a != 0).unwrap_or(fallback);
        // `(theme.getColor(diffInserted) || default).transparent(2)`: o dobro da opacidade.
        let overview = |id: &str, base: Rgba| theme.get_color(id).filter(|c| c.a != 0).unwrap_or(Rgba { a: base.a.saturating_mul(2), ..base });
        let light = matches!(theme.theme_type, ThemeType::Light | ThemeType::HighContrastLight);
        Self {
            inserted_line: line("diffEditor.insertedLineBackground", inserted_text),
            removed_line: line("diffEditor.removedLineBackground", removed_text),
            inserted_text,
            removed_text,
            diagonal_fill: color("diffEditor.diagonalFill"),
            // O padrão do tema escuro é nenhum e o oráculo desenha a linha em `widget.border`.
            border: theme.get_color("diffEditor.border").or_else(|| theme.get_color("widget.border")).unwrap_or(Rgba::opaque(0x2a, 0x2b, 0x2c)),
            overview_inserted: overview("diffEditorOverview.insertedForeground", inserted_text),
            overview_removed: overview("diffEditorOverview.removedForeground", removed_text),
            overview_background: if light { Rgba::new(0, 0, 0, 8) } else { Rgba::new(255, 255, 255, 3) },
            viewport: color("scrollbarSlider.background"),
        }
    }
}

/// O retalho da janela que cada parte do editor de diff ocupa, em px lógicos relativos ao canto do widget.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffLayout {
    pub original: Rect,
    pub gutter: Rect,
    /// A área do editor modificado (depois do 1 px de borda).
    pub modified: Rect,
    /// A linha de 1 px de `diffEditor.border` à esquerda do modificado.
    pub border: Rect,
    pub overview: Rect,
}

impl DiffLayout {
    pub fn new(width: f64, height: f64) -> Self {
        let content = (width - OVERVIEW_WIDTH).max(0.0);
        // `SashLayout._computeSashLeft`: o meio, com no mínimo 100 px para cada editor.
        let middle = (0.5 * content).floor();
        let sash_left = if content <= 200.0 { middle } else { middle.clamp(100.0, content - 100.0) };
        let original_width = (sash_left - GUTTER_WIDTH).max(0.0);
        let modified_left = sash_left;
        let modified_width = (width - modified_left - OVERVIEW_WIDTH).max(0.0);
        Self {
            original: Rect::new(0.0, 0.0, original_width, height),
            gutter: Rect::new(original_width, 0.0, sash_left, height),
            border: Rect::new(modified_left, 0.0, modified_left + 1.0, height),
            modified: Rect::new(modified_left + 1.0, 0.0, modified_left + 1.0 + modified_width, height),
            overview: Rect::new(content, 0.0, width, height),
        }
    }
}

/// Um trecho alinhado dos dois lados (`ILineRangeAlignment`), em intervalos de linhas `[início, fim)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alignment {
    pub original: (i32, i32),
    pub modified: (i32, i32),
}

/// `computeRangeAlignment` com `innerHunkAlignment` (o lado a lado) e sem espaços alheios nem quebra de linha: onde um
/// trecho que mudou tem texto igual antes ou depois da mudança na mesma linha, as linhas do original e do modificado
/// que o ladeiam se alinham ali, e o fim do trecho se alinha sempre. `original_max_column` é o `getLineMaxColumn` do
/// original e `original_lines` a quantidade de linhas dele.
pub fn compute_alignments(mappings: &[DetailedLineRangeMapping], original_lines: i32, original_max_column: &dyn Fn(i32) -> i32) -> Vec<Alignment> {
    let mut result = Vec::new();
    for m in mappings {
        let mut first = true;
        let mut last_original = m.original.start_line_number;
        let mut last_modified = m.modified.start_line_number;
        let mut emit = |original_exclusive: i32, modified_exclusive: i32, force: bool| {
            if original_exclusive < last_original || modified_exclusive < last_modified {
                return;
            }
            if first {
                first = false;
            } else if !force && (original_exclusive == last_original || modified_exclusive == last_modified) {
                // Isto realinharia uma linha já alinhada; só o alinhamento final conta.
                return;
            }
            if original_exclusive == last_original && modified_exclusive == last_modified {
                return;
            }
            result.push(Alignment { original: (last_original, original_exclusive), modified: (last_modified, modified_exclusive) });
            last_original = original_exclusive;
            last_modified = modified_exclusive;
        };
        for inner in m.inner_changes.as_deref().unwrap_or(&[]) {
            if inner.original_range.start_column > 1 && inner.modified_range.start_column > 1 {
                // Há texto sem mudança nesta linha antes da diferença.
                emit(inner.original_range.start_line_number, inner.modified_range.start_line_number, false);
            }
            let max_column = if inner.original_range.end_line_number <= original_lines { original_max_column(inner.original_range.end_line_number) } else { i32::MAX };
            if inner.original_range.end_column < max_column {
                // Há texto sem mudança nesta linha depois da diferença.
                emit(inner.original_range.end_line_number, inner.modified_range.end_line_number, false);
            }
        }
        emit(m.original.end_line_number_exclusive, m.modified.end_line_number_exclusive, true);
    }
    result
}

/// Os espaços (hachurados) que igualam a altura dos dois lados: do original o que sobra do modificado e vice-versa,
/// depois da última linha de cada trecho alinhado.
pub fn zones_of(alignments: &[Alignment], line_height: f64) -> (Vec<ViewZone>, Vec<ViewZone>) {
    let (mut original, mut modified) = (Vec::new(), Vec::new());
    for a in alignments {
        let delta = f64::from((a.modified.1 - a.modified.0) - (a.original.1 - a.original.0)) * line_height;
        if delta > 0.0 {
            original.push(ViewZone { after_line: a.original.1 - 1, height: delta });
        } else if delta < 0.0 {
            modified.push(ViewZone { after_line: a.modified.1 - 1, height: -delta });
        }
    }
    (original, modified)
}

/// As decorações do diff (`DiffEditorDecorations`) com os indicadores `+` e `-`: o fundo das linhas dos trechos, o
/// fundo mais forte nas linhas inteiras inseridas ou removidas e nos trechos alterados dentro das linhas.
pub fn decorations_of(mappings: &[DetailedLineRangeMapping], colors: &DiffColors) -> (Decorations, Decorations) {
    let (mut original_lines, mut original_ranges) = (Vec::new(), Vec::new());
    let (mut modified_lines, mut modified_ranges) = (Vec::new(), Vec::new());
    let line = |start: i32, end_exclusive: i32, background: Rgba, sign: Option<LineSign>| LineDecoration { start_line: start, end_line: end_exclusive - 1, background, margin_background: None, sign };
    for m in mappings {
        if !m.original.is_empty() {
            original_lines.push(line(m.original.start_line_number, m.original.end_line_number_exclusive, colors.removed_line, Some(LineSign::Remove)));
        }
        if !m.modified.is_empty() {
            modified_lines.push(line(m.modified.start_line_number, m.modified.end_line_number_exclusive, colors.inserted_line, Some(LineSign::Add)));
        }
        if m.modified.is_empty() || m.original.is_empty() {
            // Só removido ou só inserido: as linhas inteiras levam também o fundo do trecho.
            if !m.original.is_empty() {
                original_lines.push(line(m.original.start_line_number, m.original.end_line_number_exclusive, colors.removed_text, None));
            }
            if !m.modified.is_empty() {
                modified_lines.push(line(m.modified.start_line_number, m.modified.end_line_number_exclusive, colors.inserted_text, None));
            }
        } else {
            for inner in m.inner_changes.as_deref().unwrap_or(&[]) {
                // Sem marcadores vazios fora do intervalo de linhas do trecho.
                if m.original.contains(inner.original_range.start_line_number) {
                    original_ranges.push(RangeDecoration { range: inner.original_range, background: colors.removed_text });
                }
                if m.modified.contains(inner.modified_range.start_line_number) {
                    modified_ranges.push(RangeDecoration { range: inner.modified_range, background: colors.inserted_text });
                }
            }
        }
    }
    ((original_lines, original_ranges), (modified_lines, modified_ranges))
}

/// As decorações de um lado: as de linhas inteiras e as de trechos.
pub type Decorations = (Vec<LineDecoration>, Vec<RangeDecoration>);

/// O editor de diff: o original e o estado do diff entre ele e o modificado.
pub struct DiffEditor {
    original: EditorView,
    colors: DiffColors,
    mappings: Vec<DetailedLineRangeMapping>,
    /// As versões dos dois modelos para as quais `mappings` vale.
    computed_for: Option<(i64, i64)>,
    /// O que cada lado tinha de rolagem no último quadro (a que mudou comanda a outra).
    last_scroll: ((f64, f64), (f64, f64)),
    /// O primeiro cálculo já levou o cursor do modificado até a primeira mudança (o `revealFirstDiff` do VS Code).
    revealed_first: bool,
    /// O lado com o foco e o que está sendo arrastado.
    focus: Side,
    drag: Option<Side>,
    /// O tamanho do widget em px lógicos.
    size: (f64, f64),
}

impl DiffEditor {
    /// Cria o editor de diff do `original` contra o `modified` (que passa a ser um editor embutido, sem minimapa).
    pub fn new(original: Rc<TextModel>, modified: &mut EditorView, width: f64, height: f64, editor_colors: EditorColors, colors: DiffColors) -> Self {
        let layout = DiffLayout::new(width, height);
        let mut original = EditorView::new(original, layout.original.width(), height, editor_colors);
        original.set_embedded(true);
        original.set_read_only(true);
        original.set_focus(false);
        original.focus_requested = false;
        modified.set_embedded(true);
        modified.resize(layout.modified.width(), height);
        Self { original, colors, mappings: Vec::new(), computed_for: None, last_scroll: ((0.0, 0.0), (0.0, 0.0)), revealed_first: false, focus: Side::Modified, drag: None, size: (width, height) }
    }

    pub fn original(&self) -> &EditorView {
        &self.original
    }

    pub fn original_mut(&mut self) -> &mut EditorView {
        &mut self.original
    }

    /// As mudanças do diff (para os testes e para os comandos de navegação entre mudanças).
    pub fn mappings(&self) -> &[DetailedLineRangeMapping] {
        &self.mappings
    }

    pub fn layout(&self) -> DiffLayout {
        DiffLayout::new(self.size.0, self.size.1)
    }

    pub fn focused_side(&self) -> Side {
        self.focus
    }

    /// Troca as cores (o tema mudou).
    pub fn set_colors(&mut self, modified: &mut EditorView, editor_colors: EditorColors, colors: DiffColors) {
        self.original.set_colors(editor_colors.clone());
        modified.set_colors(editor_colors);
        self.colors = colors;
        self.computed_for = None;
    }

    /// O mapa de cores de token (a linguagem do original usa o mesmo tema).
    pub fn set_token_colors(&mut self, color_map: &[String]) {
        self.original.set_token_colors(color_map);
    }

    pub fn resize(&mut self, modified: &mut EditorView, width: f64, height: f64) {
        if self.size == (width, height) {
            return;
        }
        self.size = (width, height);
        let layout = DiffLayout::new(width, height);
        self.original.resize(layout.original.width(), height);
        modified.resize(layout.modified.width(), height);
    }

    /// O foco de cada lado: o pedido do dono do modificado (`set_focus` do shell) vale só para o lado focado, o outro
    /// perde o cursor piscando e a seleção ativa.
    fn apply_focus(&mut self, modified: &mut EditorView) {
        let requested = modified.focus_requested;
        modified.apply_focus(requested && self.focus == Side::Modified);
        self.original.apply_focus(requested && self.focus == Side::Original);
    }

    fn focus_side(&mut self, modified: &mut EditorView, side: Side) {
        self.focus = side;
        modified.focus_requested = true;
        self.apply_focus(modified);
    }

    /// Recalcula o diff se um dos textos mudou e reaplica as decorações e os espaços de alinhamento.
    pub fn update(&mut self, modified: &mut EditorView) {
        let versions = (self.original.model().get_version_id(), modified.model().get_version_id());
        if self.computed_for == Some(versions) {
            return;
        }
        self.computed_for = Some(versions);
        let options = LinesDiffComputerOptions { ignore_trim_whitespace: true, max_computation_time_ms: MAX_COMPUTATION_TIME_MS, compute_moves: false, extend_to_subwords: None };
        let diff = DefaultLinesDiffComputer::new().compute_diff(&self.original.model().get_lines_content(), &modified.model().get_lines_content(), &options);
        self.mappings = diff.changes;
        let model = self.original.model().clone();
        let alignments = compute_alignments(&self.mappings, model.get_line_count(), &|line| model.get_line_max_column(line));
        let (original_zones, modified_zones) = zones_of(&alignments, self.original.layout().line_height());
        let ((original_lines, original_ranges), (modified_lines, modified_ranges)) = decorations_of(&self.mappings, &self.colors);
        let fill = Some(self.colors.diagonal_fill);
        self.original.set_zones(original_zones, fill);
        modified.set_zones(modified_zones, fill);
        self.original.set_line_decorations(original_lines);
        self.original.set_range_decorations(original_ranges);
        modified.set_line_decorations(modified_lines);
        modified.set_range_decorations(modified_ranges);
        if !self.revealed_first {
            // Ao abrir, o cursor do modificado vai para a primeira mudança e a revela (com os espaços já postos).
            self.revealed_first = true;
            if let Some(first) = self.mappings.first() {
                modified.set_cursor(je_editor::position::Position::new(first.modified.start_line_number.clamp(1, modified.model().get_line_count()), 1));
            }
        }
    }

    /// Faz os dois lados rolarem juntos: o que se mexeu desde o último quadro leva o outro (o alinhamento deixa as
    /// linhas na mesma altura, então a rolagem vertical é a mesma; a horizontal também acompanha).
    fn sync_scroll(&mut self, modified: &mut EditorView) {
        let original = (self.original.scroll_top(), self.original.scroll_left());
        let current = (modified.scroll_top(), modified.scroll_left());
        if current != self.last_scroll.1 {
            self.original.set_scroll_top(current.0);
            self.original.scroll_by(current.1 - original.1, 0.0);
        } else if original != self.last_scroll.0 {
            modified.set_scroll_top(original.0);
            modified.scroll_by(original.1 - current.1, 0.0);
        }
        self.last_scroll = ((self.original.scroll_top(), self.original.scroll_left()), (modified.scroll_top(), modified.scroll_left()));
    }

    /// Pinta o editor de diff em `list`: `origin` é o canto do widget na janela (px lógicos) e `scale` o fator de escala.
    pub fn paint(&mut self, modified: &mut EditorView, list: &mut DisplayList, origin: (f64, f64), scale: f64, now: Instant) {
        self.update(modified);
        self.sync_scroll(modified);
        self.apply_focus(modified);
        let layout = self.layout();
        let device = |r: Rect| Rect::new(((origin.0 + r.x0) * scale).round(), ((origin.1 + r.y0) * scale).round(), ((origin.0 + r.x1) * scale).round(), ((origin.1 + r.y1) * scale).round());
        let background = self.original.colors().background;
        // A calha é só o fundo do editor (os botões de reverter ficam de fora por ora).
        list.fill_rect(device(layout.gutter), background);
        self.original.paint(list, (origin.0 + layout.original.x0, origin.1), scale, now);
        modified.paint(list, (origin.0 + layout.modified.x0, origin.1), scale, now);
        list.fill_rect(device(layout.border), self.colors.border);
        self.paint_overview(modified, list, device(layout.overview), scale);
    }

    /// A régua de visão geral: os blocos removidos do original na metade esquerda, os inseridos do modificado na direita
    /// (`OverviewZoneManager`), e o botão da janela de visão (`.diffViewport`).
    fn paint_overview(&self, modified: &EditorView, list: &mut DisplayList, area: Rect, scale: f64) {
        list.fill_rect(area, self.colors.overview_background);
        let half = (area.width() / 2.0).round();
        let height = self.size.1;
        let columns = [
            (&self.original, area.x0, self.colors.overview_removed, true),
            (modified, area.x0 + half, self.colors.overview_inserted, false),
        ];
        for (view, left, color, original) in columns {
            let layout = view.layout();
            let line_height = layout.line_height();
            let scroll_height = layout.scroll_height().floor();
            if scroll_height <= 0.0 {
                continue;
            }
            let ratio = height.floor() / scroll_height;
            let half_minimum = (OVERVIEW_MINIMUM_HEIGHT / 2.0).floor();
            for m in &self.mappings {
                let range = if original { m.original } else { m.modified };
                if range.is_empty() {
                    continue;
                }
                let offset1 = layout.line_offset(range.start_line_number);
                let offset2 = offset1 + f64::from(range.length()) * line_height;
                let (y1, y2) = ((ratio * offset1).floor(), (ratio * offset2).floor());
                let mut center = ((y1 + y2) / 2.0).floor();
                let half_height = (y2 - center).max(half_minimum);
                center = center.max(half_height).min(height.floor() - half_height);
                let top = area.y0 + (center - half_height) * scale;
                let bottom = area.y0 + (center + half_height) * scale;
                let width = if original { half } else { area.width() - half };
                list.fill_rect(Rect::new(left, top, left + width, bottom), color);
            }
        }
        let (slider_top, slider_size) = modified.scrollbar_slider();
        list.push_clip(Shape::Rect(area));
        list.fill_rect(Rect::new(area.x0, area.y0 + slider_top * scale, area.x1, area.y0 + (slider_top + slider_size) * scale), self.colors.viewport);
        list.pop_clip();
    }

    // #region entrada

    /// O lado sob `x` (relativo ao widget), se for um dos editores.
    pub fn side_at(&self, x: f64) -> Option<Side> {
        let layout = self.layout();
        if x < layout.original.x1 {
            Some(Side::Original)
        } else if x >= layout.modified.x0 && x < layout.overview.x0 {
            Some(Side::Modified)
        } else {
            None
        }
    }

    /// O botão do mouse no ponto `(x, y)` do widget: foca o lado e entrega o clique a ele; na régua, leva a janela de
    /// visão para o ponteiro.
    pub fn mouse_down(&mut self, modified: &mut EditorView, x: f64, y: f64, button: PointerButton, click_count: u32, mods: Modifiers) {
        let layout = self.layout();
        if x >= layout.overview.x0 {
            let (_, size) = modified.scrollbar_slider();
            modified.scroll_to_slider_top(y - size / 2.0);
            return;
        }
        match self.side_at(x) {
            Some(Side::Original) => {
                self.focus_side(modified, Side::Original);
                self.drag = Some(Side::Original);
                self.original.mouse_down(x - layout.original.x0, y, button, click_count, mods);
            }
            Some(Side::Modified) => {
                self.focus_side(modified, Side::Modified);
                self.drag = Some(Side::Modified);
                modified.mouse_down(x - layout.modified.x0, y, button, click_count, mods);
            }
            None => {}
        }
    }

    pub fn mouse_drag(&mut self, modified: &mut EditorView, x: f64, y: f64) {
        let layout = self.layout();
        match self.drag {
            Some(Side::Original) => self.original.mouse_drag(x - layout.original.x0, y),
            Some(Side::Modified) => modified.mouse_drag(x - layout.modified.x0, y),
            None => {}
        }
    }

    pub fn mouse_up(&mut self, modified: &mut EditorView) {
        match self.drag.take() {
            Some(Side::Original) => self.original.mouse_up(),
            Some(Side::Modified) => modified.mouse_up(),
            None => {}
        }
    }

    /// O ponteiro se moveu (ou saiu, com coordenadas negativas): os controles de dobra da margem aparecem sob ele.
    pub fn mouse_move(&mut self, modified: &mut EditorView, x: f64, y: f64) {
        let layout = self.layout();
        let side = if x < 0.0 { None } else { self.side_at(x) };
        self.original.mouse_move(if side == Some(Side::Original) { x - layout.original.x0 } else { -1.0 }, y);
        modified.mouse_move(if side == Some(Side::Modified) { x - layout.modified.x0 } else { -1.0 }, y);
    }

    /// A roda do mouse: rola o lado sob o ponteiro (a calha e a régua rolam o modificado); o outro acompanha no quadro.
    pub fn scroll_by(&mut self, modified: &mut EditorView, x: f64, dx: f64, dy: f64) {
        if self.side_at(x) == Some(Side::Original) {
            self.original.scroll_by(dx, dy);
        } else {
            modified.scroll_by(dx, dy);
        }
    }

    /// Uma tecla de navegação ou de comando para o lado focado.
    pub fn key_down(&mut self, modified: &mut EditorView, key: EditorKey, mods: Modifiers) -> bool {
        match self.focus {
            Side::Original => self.original.key_down(key, mods),
            Side::Modified => modified.key_down(key, mods),
        }
    }

    /// Texto digitado: o original é somente leitura e ignora.
    pub fn type_text(&mut self, modified: &mut EditorView, text: &str) {
        match self.focus {
            Side::Original => self.original.type_text(text),
            Side::Modified => modified.type_text(text),
        }
    }

    // #endregion
}
