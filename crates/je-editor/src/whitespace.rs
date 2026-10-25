//! Porte de `src/vs/editor/browser/viewParts/whitespace/whitespace.ts`: o `WhitespaceOverlay`, que desenha os
//! espaços e as tabulações de cada linha da visão conforme `editor.renderWhitespace` (`boundary`, `selection`,
//! `trailing`, `all`), em um único caminho SVG por linha (`experimentalWhitespaceRendering: 'svg'`) ou em
//! marcadores de fonte (`'font'`).
//!
//! Divergências:
//! - `render` devolve o HTML da linha no original. A UI é nativa, sem DOM: o resultado de cada linha é uma
//!   [`WhitespaceLine`] (o que o `style`, o `viewBox`, o `fill` e o atributo `d` do `path` dizem, no caso do SVG, ou
//!   a lista de `div.mwh` com `left` e `height`, no caso da fonte) e a linha sem nada é [`WhitespaceLine::Empty`]
//!   (a string vazia). O `whitespace.css` é só especificação de estilo, que o pintor nativo lê ao desenhar.
//! - `${color}` do `fill` é a cor do tema em texto (`Color::to_string`); sem cor no tema o original escreve `null`.
//! - `Number.prototype.toFixed(2)` não tem equivalente direto: [`to_fixed_2`] reproduz o arredondamento dele (empate
//!   exato vai para longe do zero, ao contrário do `format!` de Rust, que vai para o par).
//! - `ViewEventHandler extends Disposable`: o descarte é o [`IDisposable`] à parte (ver `view_events.rs`). Como o
//!   construtor se registra no `ViewContext` (`addEventHandler(this)`), [`WhitespaceOverlay::new`] devolve um
//!   `Rc<Self>` e guarda uma referência fraca a si mesmo para `removeEventHandler(this)` no `dispose`.
//! - O estado mutável (`_options`, `_selection`, `_renderResult`) fica em `RefCell`, porque o despachante
//!   compartilha o handler.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::char_code as cc;
use je_base::lifecycle::IDisposable;
use je_base::strings;

use crate::config::editor_options::EditorOption;
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::registry::{
    EXPERIMENTAL_WHITESPACE_RENDERING, RENDER_WHITESPACE, STOP_RENDERING_LINE_AFTER,
};
use crate::cursor_common::EditorConfiguration;
use crate::dynamic_view_overlay::DynamicViewOverlay;
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::rendering_context::RenderingContext;
use crate::selection::Selection;
use crate::view_context::ViewContext;
use crate::view_events::{
    ShouldRenderState, ViewConfigurationChangedEvent, ViewCursorStateChangedEvent, ViewDecorationsChangedEvent,
    ViewEventHandler, ViewFlushedEvent, ViewLinesChangedEvent, ViewLinesDeletedEvent, ViewLinesInsertedEvent,
    ViewScrollChangedEvent, ViewZonesChangedEvent,
};
use crate::view_line_renderer::RenderWhitespace;
use crate::view_model_types::{IViewModel, ViewLineRenderingData};

/// U+2E31 (WORD SEPARATOR MIDDLE DOT).
const WORD_SEPARATOR_MIDDLE_DOT: char = '\u{2E31}';
/// U+00B7 (MIDDLE DOT).
const MIDDLE_DOT: char = '\u{B7}';
/// U+FFEB (HALFWIDTH RIGHTWARDS ARROW).
const HALFWIDTH_RIGHTWARDS_ARROW: char = '\u{FFEB}';
/// U+2192 (RIGHTWARDS ARROW).
const RIGHTWARDS_ARROW: char = '\u{2192}';

/// O `<svg>` de uma linha: a caixa (`width` e `height`, que também formam o `viewBox` `0 0 width height`), a cor de
/// preenchimento e o atributo `d` do único `<path>` (vazio quando a linha não tem nada a desenhar, e então o `<path>`
/// não existe).
#[derive(Clone, Debug, PartialEq)]
pub struct WhitespaceSvg {
    pub width: f64,
    pub height: f64,
    /// O `fill` (`null` quando o tema não tem a cor, como o `${color}` do original).
    pub fill: String,
    /// O `d` do `<path>`, uma sequência de subcaminhos fechados (`M ... Z `), um por espaço ou tab.
    pub path: String,
}

impl WhitespaceSvg {
    /// O atributo `viewBox`.
    pub fn view_box(&self) -> String {
        format!("0 0 {} {}", self.width, self.height)
    }
}

/// O `<div class="mwh">` de um espaço ou tab renderizado pela fonte.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhitespaceMarker {
    pub left: f64,
    pub height: f64,
    pub glyph: char,
}

/// O que `render` devolve para uma linha da visão.
#[derive(Clone, Debug, PartialEq)]
pub enum WhitespaceLine {
    /// A string vazia: a linha não pinta nada.
    Empty,
    /// O renderizador SVG (`experimentalWhitespaceRendering: 'svg'`).
    Svg(WhitespaceSvg),
    /// O renderizador por fonte (`'font'`), um marcador por espaço ou tab.
    Markers(Vec<WhitespaceMarker>),
}

/// `Number.prototype.toFixed(2)`: arredonda para o valor de duas casas mais próximo, e o empate exato vai para o
/// maior (longe do zero, porque o sinal é tratado à parte).
fn to_fixed_2(value: f64) -> String {
    let magnitude = value.abs();
    let exact = format!("{magnitude:.60}");
    let is_tie = exact
        .split_once('.')
        .and_then(|(_, fraction)| fraction.get(2..))
        .and_then(|tail| tail.strip_prefix('5'))
        .is_some_and(|rest| rest.bytes().all(|digit| digit == b'0'));
    // O próximo double acima de um empate exato já não é empate e arredonda para cima.
    let rounded = if is_tie { f64::from_bits(magnitude.to_bits() + 1) } else { magnitude };
    let sign = if value < 0.0 { "-" } else { "" };
    format!("{sign}{rounded:.2}")
}

/// `Number(x.toFixed(2))`.
fn round_to_2(value: f64) -> f64 {
    to_fixed_2(value).parse().unwrap_or(f64::NAN)
}

/// `Math.round` do JavaScript (a metade arredonda para cima).
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `_renderCircle`.
fn render_circle(line_height: f64, space_width: f64, left: f64) -> String {
    let radius = round_to_2(space_width / 7.0);
    let center_x = round_to_2(left + space_width / 2.0);
    let center_y = to_fixed_2(line_height / 2.0);
    let diameter = to_fixed_2(radius * 2.0);
    format!(
        "M {} {center_y} a {radius} {radius} 0 1 0 {diameter} 0 a {radius} {radius} 0 1 0 -{diameter} 0 Z ",
        to_fixed_2(center_x - radius)
    )
}

/// `_renderArrow`.
fn render_arrow(line_height: f64, space_width: f64, left: f64) -> String {
    let stroke_width = space_width / 7.0;
    let width = space_width;
    let dy = line_height / 2.0;
    let dx = left;

    let p1 = (0.0, stroke_width / 2.0);
    let p2 = (100.0 / 125.0 * width, p1.1);
    let p3 = (p2.0 - 0.2 * p2.0, p2.1 + 0.2 * p2.0);
    let p4 = (p3.0 + 0.1 * p2.0, p3.1 + 0.1 * p2.0);
    let p5 = (p4.0 + 0.35 * p2.0, p4.1 - 0.35 * p2.0);
    let p6 = (p5.0, -p5.1);
    let p7 = (p4.0, -p4.1);
    let p8 = (p3.0, -p3.1);
    let p9 = (p2.0, -p2.1);
    let p10 = (p1.0, -p1.1);

    let parts = [p1, p2, p3, p4, p5, p6, p7, p8, p9, p10]
        .iter()
        .map(|(x, y)| format!("{} {}", to_fixed_2(dx + x), to_fixed_2(dy + y)))
        .collect::<Vec<_>>()
        .join(" L ");
    format!("M {parts} Z ")
}

/// O `WhitespaceOverlay`.
pub struct WhitespaceOverlay<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> {
    context: Rc<ViewContext<VM>>,
    options: RefCell<WhitespaceOptions>,
    selection: RefCell<Vec<Selection>>,
    render_result: RefCell<Option<Vec<WhitespaceLine>>>,
    should_render_state: ShouldRenderState,
    this: Weak<Self>,
}

impl<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> WhitespaceOverlay<VM> {
    pub fn new(context: Rc<ViewContext<VM>>) -> Rc<Self> {
        let options = WhitespaceOptions::new(&*context.configuration);
        let overlay = Rc::new_cyclic(|this| Self {
            context,
            options: RefCell::new(options),
            selection: RefCell::new(Vec::new()),
            render_result: RefCell::new(None),
            should_render_state: ShouldRenderState::default(),
            this: this.clone(),
        });
        let handler: Rc<dyn ViewEventHandler> = overlay.clone();
        overlay.context.add_event_handler(handler);
        overlay
    }

    /// `_applyRenderWhitespace`.
    fn apply_render_whitespace(
        &self,
        ctx: &RenderingContext<VM>,
        line_number: i32,
        selections: Option<&[OffsetRange]>,
        line_data: &ViewLineRenderingData,
    ) -> WhitespaceLine {
        let options = *self.options.borrow();
        if options.render_whitespace == RenderWhitespace::Selection && selections.is_none() {
            return WhitespaceLine::Empty;
        }
        if options.render_whitespace == RenderWhitespace::Trailing && line_data.continues_with_wrapped_line {
            return WhitespaceLine::Empty;
        }
        let color = self.context.theme.get_color(WHITESPACE_COLOR);
        let use_svg = options.render_with_svg;

        let line_content = line_data.content.units();
        let len = if options.stop_rendering_line_after == -1.0 {
            line_content.len()
        } else {
            (options.stop_rendering_line_after.max(0.0) as usize).min(line_content.len())
        };
        let continues_with_wrapped_line = line_data.continues_with_wrapped_line;
        let faux_indent_length = usize::try_from(line_data.min_column - 1).unwrap_or(0);
        let only_boundary = options.render_whitespace == RenderWhitespace::Boundary;
        let only_trailing = options.render_whitespace == RenderWhitespace::Trailing;
        let line_height = ctx.get_line_height_for_line_number(line_number);
        let space_width = options.space_width;
        let wsmiddot_diff = (options.wsmiddot_width - space_width).abs();
        let middot_diff = (options.middot_width - space_width).abs();

        let render_space_char = if wsmiddot_diff < middot_diff { WORD_SEPARATOR_MIDDLE_DOT } else { MIDDLE_DOT };
        let tab_char =
            if options.can_use_halfwidth_rightwards_arrow { HALFWIDTH_RIGHTWARDS_ARROW } else { RIGHTWARDS_ARROW };

        let mut path = String::new();
        let mut markers = Vec::new();

        let mut line_is_empty_or_whitespace = false;
        let first_non_whitespace_index;
        let last_non_whitespace_index;
        match strings::first_non_whitespace_index(line_content) {
            None => {
                line_is_empty_or_whitespace = true;
                first_non_whitespace_index = len;
                last_non_whitespace_index = len;
            }
            Some(first) => {
                first_non_whitespace_index = first;
                last_non_whitespace_index =
                    strings::last_non_whitespace_index(line_content, line_content.len() as i64 - 1).unwrap_or(0);
            }
        }

        let mut current_selection_index = 0;
        let mut max_left = 0.0_f64;

        for char_index in faux_indent_length..len {
            let ch_code = line_content[char_index];

            let current_selection_ended = selections.is_some_and(|selections| {
                selections.get(current_selection_index).is_some_and(|current| current.end_exclusive <= char_index as i32)
            });
            if current_selection_ended {
                current_selection_index += 1;
            }

            if ch_code != cc::Tab && ch_code != cc::Space {
                continue;
            }

            if only_trailing && !line_is_empty_or_whitespace && char_index <= last_non_whitespace_index {
                // Se só o espaço em branco do fim é desenhado, o índice precisa apontar para ele.
                continue;
            }

            if only_boundary
                && char_index >= first_non_whitespace_index
                && char_index <= last_non_whitespace_index
                && ch_code == cc::Space
            {
                // Só o espaço em branco de fronteira é desenhado.
                let prev = char_index.checked_sub(1).map(|index| line_content[index]);
                let next = (char_index + 1 < len).then(|| line_content[char_index + 1]);
                if prev != Some(cc::Space) && next != Some(cc::Space) {
                    continue;
                }
            }

            if only_boundary && continues_with_wrapped_line && char_index == len - 1 {
                let prev = char_index.checked_sub(1).map(|index| line_content[index]);
                let is_single_trailing_space =
                    ch_code == cc::Space && prev != Some(cc::Space) && prev != Some(cc::Tab);
                if is_single_trailing_space {
                    continue;
                }
            }

            if let Some(selections) = selections {
                // Com `selection`, o índice precisa cair dentro de uma seleção.
                let inside_selection = selections.get(current_selection_index).is_some_and(|current| {
                    current.start <= char_index as i32 && (char_index as i32) < current.end_exclusive
                });
                if !inside_selection {
                    continue;
                }
            }

            let Some(visible_range) = ctx.visible_range_for_position(Position::new(line_number, char_index as i32 + 1))
            else {
                continue;
            };

            if use_svg {
                max_left = max_left.max(visible_range.left);
                if ch_code == cc::Tab {
                    path.push_str(&render_arrow(line_height, space_width, visible_range.left));
                } else {
                    path.push_str(&render_circle(line_height, space_width, visible_range.left));
                }
            } else {
                markers.push(WhitespaceMarker {
                    left: visible_range.left,
                    height: line_height,
                    glyph: if ch_code == cc::Tab { tab_char } else { render_space_char },
                });
            }
        }

        if use_svg {
            return WhitespaceLine::Svg(WhitespaceSvg {
                width: js_round(max_left + space_width),
                height: line_height,
                fill: color.map_or_else(|| "null".to_owned(), |color| color.to_string()),
                path,
            });
        }

        WhitespaceLine::Markers(markers)
    }
}

/// `editorWhitespaces` (`editorWhitespace.foreground`).
const WHITESPACE_COLOR: &str = "editorWhitespace.foreground";

impl<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> IDisposable for WhitespaceOverlay<VM> {
    fn dispose(&self) {
        if let Some(this) = self.this.upgrade() {
            let handler: Rc<dyn ViewEventHandler> = this;
            self.context.remove_event_handler(&handler);
        }
        *self.render_result.borrow_mut() = None;
    }
}

impl<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> ViewEventHandler for WhitespaceOverlay<VM> {
    fn should_render_state(&self) -> &ShouldRenderState {
        &self.should_render_state
    }

    // --- begin event handlers

    fn on_configuration_changed(&self, e: &ViewConfigurationChangedEvent) -> bool {
        let new_options = WhitespaceOptions::new(&*self.context.configuration);
        if *self.options.borrow() == new_options {
            return e.has_changed(EditorOption::LayoutInfo);
        }
        *self.options.borrow_mut() = new_options;
        true
    }
    fn on_cursor_state_changed(&self, e: &ViewCursorStateChangedEvent) -> bool {
        *self.selection.borrow_mut() = e.selections.clone();
        self.options.borrow().render_whitespace == RenderWhitespace::Selection
    }
    fn on_decorations_changed(&self, _e: &ViewDecorationsChangedEvent) -> bool {
        true
    }
    fn on_flushed(&self, _e: &ViewFlushedEvent) -> bool {
        true
    }
    fn on_lines_changed(&self, _e: &ViewLinesChangedEvent) -> bool {
        true
    }
    fn on_lines_deleted(&self, _e: &ViewLinesDeletedEvent) -> bool {
        true
    }
    fn on_lines_inserted(&self, _e: &ViewLinesInsertedEvent) -> bool {
        true
    }
    fn on_scroll_changed(&self, e: &ViewScrollChangedEvent) -> bool {
        e.scroll_top_changed
    }
    fn on_zones_changed(&self, _e: &ViewZonesChangedEvent) -> bool {
        true
    }
    // --- end event handlers
}

impl<VM: IViewModel<EventHandler = dyn ViewEventHandler> + 'static> DynamicViewOverlay<VM> for WhitespaceOverlay<VM> {
    type Output = WhitespaceLine;

    fn prepare_render(&self, ctx: &RenderingContext<VM>) {
        let render_whitespace = self.options.borrow().render_whitespace;
        if render_whitespace == RenderWhitespace::None {
            *self.render_result.borrow_mut() = None;
            return;
        }

        let viewport_data = &ctx.viewport_data;
        let mut result = Vec::new();
        for line_number in viewport_data.start_line_number..=viewport_data.end_line_number {
            let line_data = self.context.view_model.get_view_line_rendering_data(line_number);

            let mut selections_on_line: Option<Vec<OffsetRange>> = None;
            if render_whitespace == RenderWhitespace::Selection {
                for selection in self.selection.borrow().iter() {
                    if selection.end_line_number < line_number || selection.start_line_number > line_number {
                        // A seleção não cruza a linha.
                        continue;
                    }

                    let start_column =
                        if selection.start_line_number == line_number { selection.start_column } else { line_data.min_column };
                    let end_column =
                        if selection.end_line_number == line_number { selection.end_column } else { line_data.max_column };

                    if start_column < end_column {
                        selections_on_line
                            .get_or_insert_with(Vec::new)
                            .push(OffsetRange::new(start_column - 1, end_column - 1));
                    }
                }
            }

            result.push(self.apply_render_whitespace(ctx, line_number, selections_on_line.as_deref(), &line_data));
        }
        *self.render_result.borrow_mut() = Some(result);
    }

    fn render(&self, start_line_number: i32, line_number: i32) -> WhitespaceLine {
        let render_result = self.render_result.borrow();
        let Some(render_result) = render_result.as_ref() else {
            return WhitespaceLine::Empty;
        };
        usize::try_from(line_number - start_line_number)
            .ok()
            .and_then(|line_index| render_result.get(line_index))
            .cloned()
            .unwrap_or(WhitespaceLine::Empty)
    }
}

/// O subconjunto da configuração do editor que a parte lê, copiado para distinguir uma mudança de configuração de
/// uma que deixa o resultado pintado intacto (`WhitespaceOptions`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct WhitespaceOptions {
    render_whitespace: RenderWhitespace,
    render_with_svg: bool,
    space_width: f64,
    middot_width: f64,
    wsmiddot_width: f64,
    can_use_halfwidth_rightwards_arrow: bool,
    line_height: f64,
    stop_rendering_line_after: f64,
}

impl WhitespaceOptions {
    fn new(config: &dyn EditorConfiguration) -> Self {
        let options = config.options();
        let font_info = options.get(FONT_INFO);
        let experimental_whitespace_rendering = options.get(EXPERIMENTAL_WHITESPACE_RENDERING);
        let (render_whitespace, render_with_svg) = match experimental_whitespace_rendering.as_str() {
            // O espaço em branco é desenhado na própria linha.
            "off" => (RenderWhitespace::None, false),
            "svg" => (RenderWhitespace::from_option(options.get(RENDER_WHITESPACE)), true),
            _ => (RenderWhitespace::from_option(options.get(RENDER_WHITESPACE)), false),
        };
        Self {
            render_whitespace,
            render_with_svg,
            space_width: font_info.space_width,
            middot_width: font_info.middot_width,
            wsmiddot_width: font_info.wsmiddot_width,
            can_use_halfwidth_rightwards_arrow: font_info.can_use_halfwidth_rightwards_arrow,
            line_height: *options.get(LINE_HEIGHT),
            stop_rendering_line_after: *options.get(STOP_RENDERING_LINE_AFTER),
        }
    }
}
