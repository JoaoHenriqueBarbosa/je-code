//! Porte de `EditorLayoutInfoComputer` e dos tipos de entrada dele (`editorOptions.ts`, linhas 2605 a 3069):
//! o cálculo da geometria do editor, com minimapa, margem de glifos, números de linha, decorações,
//! quebra de linha e régua de visão geral.
//!
//! O resultado é o [`EditorLayoutInfo`] de `layout_info.rs`. Decisões de tradução:
//!
//! - O original lê `env.memory || new ComputeOptionsMemory()`: o ambiente guarda a memória em
//!   `Rc<RefCell<...>>` (`None` é o `null`), e quando não há memória usa-se uma nova, descartada no fim.
//! - `Math.max` e `Math.min` do JavaScript propagam `NaN`, e `f64::max`/`f64::min` não: por isso o
//!   arquivo tem `js_max` e `js_min`. `Math.round` só recebe valores não negativos aqui, onde
//!   `f64::round` coincide com ele.
//! - `IEditorLayoutComputerInput` existe no original sem uso no cálculo (o computador lê as opções direto de
//!   `IComputedEditorOptions`); aqui ele é o [`EditorLayoutComputerInput`], só como tipo.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;

use crate::config::editor_options::accessibility_support::ACCESSIBILITY_SUPPORT;
use crate::config::editor_options::layout_info::{
    EditorLayoutInfo, EditorMinimapLayoutInfo, OverviewRulerPosition, RenderMinimap,
};
use crate::config::editor_options::line_decorations_width::LINE_DECORATIONS_WIDTH;
use crate::config::editor_options::line_numbers::{
    InternalEditorRenderLineNumbersOptions, RenderLineNumbersType, LINE_NUMBERS,
};
use crate::config::editor_options::minimap::{EditorMinimapOptions, MinimapSide, MinimapSize, MINIMAP};
use crate::config::editor_options::padding::PADDING;
use crate::config::editor_options::scrollbar::{InternalEditorScrollbarOptions, SCROLLBAR};
use crate::config::editor_options::simple_keys::{
    ShowFoldingControls, WordWrap, WordWrapOverride, FOLDING, GLYPH_MARGIN, LINE_NUMBERS_MIN_CHARS,
    SCROLL_BEYOND_LAST_LINE, SHOW_FOLDING_CONTROLS, WORD_WRAP, WORD_WRAP_COLUMN, WORD_WRAP_INDICATOR,
    WORD_WRAP_OVERRIDE_1, WORD_WRAP_OVERRIDE_2,
};
use crate::config::editor_options::{
    to_int32, AccessibilitySupport, ComputeOptionsMemory, ComputedEditorOption, ComputedEditorOptions, EditorOption,
    EditorOptionImpl, EnvironmentalOptions,
};

/// `MINIMAP_GUTTER_WIDTH` (`editorOptions.ts`, linha 884): a folga, em px, que o minimapa soma à própria largura.
pub const MINIMAP_GUTTER_WIDTH: f64 = 8.0;

/// `Math.max(a, b)`: `NaN` se algum dos dois é `NaN`.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// `Math.min(a, b)`: `NaN` se algum dos dois é `NaN`.
fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// `lineDecorationsWidth: string | number` de [`EditorLayoutComputerInput`].
#[derive(Clone, Debug, PartialEq)]
pub enum LineDecorationsWidthInput {
    Number(f64),
    Text(String),
}

/// `IEditorLayoutComputerInput`.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorLayoutComputerInput {
    pub outer_width: f64,
    pub outer_height: f64,
    pub is_dominated_by_long_lines: bool,
    pub line_height: f64,
    pub line_numbers_digit_count: f64,
    pub typical_halfwidth_character_width: f64,
    pub max_digit_width: f64,
    pub pixel_ratio: f64,
    pub glyph_margin: bool,
    pub line_decorations_width: LineDecorationsWidthInput,
    pub folding: bool,
    pub minimap: EditorMinimapOptions,
    pub scrollbar: InternalEditorScrollbarOptions,
    pub line_numbers: InternalEditorRenderLineNumbersOptions,
    pub line_numbers_min_chars: f64,
    pub scroll_beyond_last_line: bool,
    pub word_wrap: WordWrap,
    pub word_wrap_column: f64,
    pub word_wrap_minified: bool,
    pub accessibility_support: AccessibilitySupport,
}

/// `IMinimapLayoutInput`.
#[derive(Clone, Debug, PartialEq)]
pub struct MinimapLayoutInput {
    pub outer_width: f64,
    pub outer_height: f64,
    pub line_height: f64,
    pub typical_halfwidth_character_width: f64,
    pub pixel_ratio: f64,
    pub scroll_beyond_last_line: bool,
    pub padding_top: f64,
    pub padding_bottom: f64,
    pub minimap: EditorMinimapOptions,
    pub vertical_scrollbar_width: f64,
    pub view_line_count: f64,
    pub remaining_width: f64,
    pub is_viewport_wrapping: bool,
}

/// `EditorLayoutInfoComputerEnv`.
#[derive(Clone, Debug)]
pub struct EditorLayoutInfoComputerEnv {
    pub memory: Option<Rc<RefCell<ComputeOptionsMemory>>>,
    pub outer_width: f64,
    pub outer_height: f64,
    pub is_dominated_by_long_lines: bool,
    pub line_height: f64,
    pub view_line_count: f64,
    pub line_numbers_digit_count: f64,
    pub typical_halfwidth_character_width: f64,
    pub max_digit_width: f64,
    pub pixel_ratio: f64,
    pub glyph_margin_decoration_lane_count: f64,
}

/// A entrada de `computeContainedMinimapLineCount`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContainedMinimapLineCountInput {
    pub view_line_count: f64,
    pub scroll_beyond_last_line: bool,
    pub padding_top: f64,
    pub padding_bottom: f64,
    pub height: f64,
    pub line_height: f64,
    pub pixel_ratio: f64,
}

/// O resultado de `computeContainedMinimapLineCount`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContainedMinimapLineCount {
    pub typical_viewport_line_count: f64,
    pub extra_lines_before_first_line: f64,
    pub extra_lines_beyond_last_line: f64,
    pub desired_ratio: f64,
    pub minimap_line_count: f64,
}

/// O valor inicial de `layoutInfo` (o `defaultValue` do construtor).
fn default_layout_info() -> EditorLayoutInfo {
    EditorLayoutInfo {
        width: 0.0,
        height: 0.0,
        glyph_margin_left: 0.0,
        glyph_margin_width: 0.0,
        glyph_margin_decoration_lane_count: 0.0,
        line_numbers_left: 0.0,
        line_numbers_width: 0.0,
        decorations_left: 0.0,
        decorations_width: 0.0,
        content_left: 0.0,
        content_width: 0.0,
        minimap: EditorMinimapLayoutInfo {
            render_minimap: RenderMinimap::None,
            minimap_left: 0.0,
            minimap_width: 0.0,
            minimap_height_is_editor_height: false,
            minimap_is_sampling: false,
            minimap_scale: 1.0,
            minimap_line_height: 1.0,
            minimap_canvas_inner_width: 0.0,
            minimap_canvas_inner_height: 0.0,
            minimap_canvas_outer_width: 0.0,
            minimap_canvas_outer_height: 0.0,
        },
        viewport_column: 0.0,
        is_word_wrap_minified: false,
        is_viewport_wrapping: false,
        wrapping_column: -1.0,
        vertical_scrollbar_width: 0.0,
        horizontal_scrollbar_height: 0.0,
        overview_ruler: OverviewRulerPosition { top: 0.0, width: 0.0, height: 0.0, right: 0.0 },
    }
}

/// O `env` de `compute`, repassado a `computeLayout` com os campos que ele lê.
fn layout_env(env: &EnvironmentalOptions) -> EditorLayoutInfoComputerEnv {
    EditorLayoutInfoComputerEnv {
        memory: env.memory.clone(),
        outer_width: env.outer_width,
        outer_height: env.outer_height,
        is_dominated_by_long_lines: env.is_dominated_by_long_lines,
        line_height: env.font_info.line_height,
        view_line_count: env.view_line_count,
        line_numbers_digit_count: env.line_numbers_digit_count,
        typical_halfwidth_character_width: env.font_info.typical_halfwidth_character_width,
        max_digit_width: env.font_info.max_digit_width,
        pixel_ratio: env.pixel_ratio,
        glyph_margin_decoration_lane_count: env.glyph_margin_decoration_lane_count,
    }
}

/// `EditorLayoutInfoComputer`: opção só calculada (`name = "_never_"`, sem esquema).
pub struct EditorLayoutInfoComputer {
    inner: ComputedEditorOption<EditorLayoutInfo>,
}

impl EditorLayoutInfoComputer {
    pub fn new() -> Self {
        Self {
            inner: ComputedEditorOption::new(EditorOption::LayoutInfo, default_layout_info(), |env, options, _| {
                Self::compute_layout(options, &layout_env(env))
            }),
        }
    }

    /// `computeContainedMinimapLineCount`.
    pub fn compute_contained_minimap_line_count(input: &ContainedMinimapLineCountInput) -> ContainedMinimapLineCount {
        let typical_viewport_line_count = input.height / input.line_height;
        let extra_lines_before_first_line = (input.padding_top / input.line_height).floor();
        let mut extra_lines_beyond_last_line = (input.padding_bottom / input.line_height).floor();
        if input.scroll_beyond_last_line {
            extra_lines_beyond_last_line = js_max(extra_lines_beyond_last_line, typical_viewport_line_count - 1.0);
        }
        let desired_ratio =
            (extra_lines_before_first_line + input.view_line_count + extra_lines_beyond_last_line) / (input.pixel_ratio * input.height);
        let minimap_line_count = (input.view_line_count / desired_ratio).floor();
        ContainedMinimapLineCount {
            typical_viewport_line_count,
            extra_lines_before_first_line,
            extra_lines_beyond_last_line,
            desired_ratio,
            minimap_line_count,
        }
    }

    /// `_computeMinimapLayout`.
    fn compute_minimap_layout(input: &MinimapLayoutInput, memory: &mut ComputeOptionsMemory) -> EditorMinimapLayoutInfo {
        let outer_width = input.outer_width;
        let outer_height = input.outer_height;
        let pixel_ratio = input.pixel_ratio;

        if !input.minimap.enabled {
            return EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::None,
                minimap_left: 0.0,
                minimap_width: 0.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 1.0,
                minimap_line_height: 1.0,
                minimap_canvas_inner_width: 0.0,
                minimap_canvas_inner_height: (pixel_ratio * outer_height).floor(),
                minimap_canvas_outer_width: 0.0,
                minimap_canvas_outer_height: outer_height,
            };
        }

        // Pode usar a memória se só `viewLineCount` e `remainingWidth` mudaram. `outerWidth`, `viewLineCount` e
        // `remainingWidth` ficam de fora da comparação de propósito, como no original.
        let could_use_memory = memory.stable_minimap_layout_input.as_ref().is_some_and(|stable| {
            input.outer_height == stable.outer_height
                && input.line_height == stable.line_height
                && input.typical_halfwidth_character_width == stable.typical_halfwidth_character_width
                && input.pixel_ratio == stable.pixel_ratio
                && input.scroll_beyond_last_line == stable.scroll_beyond_last_line
                && input.padding_top == stable.padding_top
                && input.padding_bottom == stable.padding_bottom
                && input.minimap.enabled == stable.minimap.enabled
                && input.minimap.side == stable.minimap.side
                && input.minimap.size == stable.minimap.size
                && input.minimap.show_slider == stable.minimap.show_slider
                && input.minimap.render_characters == stable.minimap.render_characters
                && input.minimap.max_column == stable.minimap.max_column
                && input.minimap.scale == stable.minimap.scale
                && input.vertical_scrollbar_width == stable.vertical_scrollbar_width
                && input.is_viewport_wrapping == stable.is_viewport_wrapping
        });

        let line_height = input.line_height;
        let typical_halfwidth_character_width = input.typical_halfwidth_character_width;
        let scroll_beyond_last_line = input.scroll_beyond_last_line;
        let minimap_render_characters = input.minimap.render_characters;
        let mut minimap_scale = if pixel_ratio >= 2.0 { (input.minimap.scale * 2.0).round() } else { input.minimap.scale };
        let minimap_max_column = input.minimap.max_column;
        let minimap_size = input.minimap.size;
        let minimap_side = input.minimap.side;
        let vertical_scrollbar_width = input.vertical_scrollbar_width;
        let view_line_count = input.view_line_count;
        let remaining_width = input.remaining_width;
        let is_viewport_wrapping = input.is_viewport_wrapping;

        let base_char_height = if minimap_render_characters { 2.0 } else { 3.0 };
        let mut minimap_canvas_inner_height = (pixel_ratio * outer_height).floor();
        let minimap_canvas_outer_height = minimap_canvas_inner_height / pixel_ratio;
        let mut minimap_height_is_editor_height = false;
        let mut minimap_is_sampling = false;
        let mut minimap_line_height = base_char_height * minimap_scale;
        let mut minimap_char_width = minimap_scale / pixel_ratio;
        let mut minimap_width_multiplier = 1.0;

        if matches!(minimap_size, MinimapSize::Fill | MinimapSize::Fit) {
            let ContainedMinimapLineCount {
                typical_viewport_line_count,
                extra_lines_before_first_line,
                extra_lines_beyond_last_line,
                desired_ratio,
                minimap_line_count,
            } = Self::compute_contained_minimap_line_count(&ContainedMinimapLineCountInput {
                view_line_count,
                scroll_beyond_last_line,
                padding_top: input.padding_top,
                padding_bottom: input.padding_bottom,
                height: outer_height,
                line_height,
                pixel_ratio,
            });
            // A razão não faz parte do layout de propósito, para ele não mudar o tempo todo na amostragem.
            let ratio = view_line_count / minimap_line_count;

            if ratio > 1.0 {
                minimap_height_is_editor_height = true;
                minimap_is_sampling = true;
                minimap_scale = 1.0;
                minimap_line_height = 1.0;
                minimap_char_width = minimap_scale / pixel_ratio;
            } else {
                let mut fit_becomes_fill = false;
                let mut max_minimap_scale = minimap_scale + 1.0;

                if minimap_size == MinimapSize::Fit {
                    let effective_minimap_height =
                        ((extra_lines_before_first_line + view_line_count + extra_lines_beyond_last_line) * minimap_line_height).ceil();
                    if is_viewport_wrapping && could_use_memory && remaining_width <= memory.stable_fit_remaining_width {
                        // Há um laço com `fit` e quebra pela viewport: a quantidade de linhas da vista muda o
                        // layout do minimapa, que muda a largura da viewport, que muda a quantidade de
                        // linhas. Para quebrar o laço, depois de descer para uma escala menor, ficamos nela.
                        fit_becomes_fill = true;
                        max_minimap_scale = memory.stable_fit_max_minimap_scale;
                    } else {
                        fit_becomes_fill = effective_minimap_height > minimap_canvas_inner_height;
                    }
                }

                if minimap_size == MinimapSize::Fill || fit_becomes_fill {
                    minimap_height_is_editor_height = true;
                    let configured_minimap_scale = minimap_scale;
                    minimap_line_height = js_min(line_height * pixel_ratio, js_max(1.0, (1.0 / desired_ratio).floor()));
                    if is_viewport_wrapping && could_use_memory && remaining_width <= memory.stable_fit_remaining_width {
                        // O mesmo laço, agora com `fill` e quebra pela viewport.
                        max_minimap_scale = memory.stable_fit_max_minimap_scale;
                    }
                    minimap_scale = js_min(max_minimap_scale, js_max(1.0, (minimap_line_height / base_char_height).floor()));
                    if minimap_scale > configured_minimap_scale {
                        minimap_width_multiplier = js_min(2.0, minimap_scale / configured_minimap_scale);
                    }
                    minimap_char_width = minimap_scale / pixel_ratio / minimap_width_multiplier;
                    minimap_canvas_inner_height = (js_max(
                        typical_viewport_line_count,
                        extra_lines_before_first_line + view_line_count + extra_lines_beyond_last_line,
                    ) * minimap_line_height)
                        .ceil();
                    if is_viewport_wrapping {
                        // Lembra para a próxima vez.
                        memory.stable_minimap_layout_input = Some(input.clone());
                        memory.stable_fit_remaining_width = remaining_width;
                        memory.stable_fit_max_minimap_scale = minimap_scale;
                    } else {
                        memory.stable_minimap_layout_input = None;
                        memory.stable_fit_remaining_width = 0.0;
                    }
                }
            }
        }

        // Dado (deixando 2px para o cursor ter espaço depois do último caractere):
        // viewportColumn = (contentWidth - verticalScrollbarWidth - 2) / typicalHalfwidthCharacterWidth
        // minimapWidth = viewportColumn * minimapCharWidth
        // contentWidth = remainingWidth - minimapWidth
        // Quais são bons valores para contentWidth e minimapWidth?
        //
        // minimapWidth = ((remainingWidth - verticalScrollbarWidth - 2) * minimapCharWidth)
        //                / (typicalHalfwidthCharacterWidth + minimapCharWidth)

        let minimap_max_width = (minimap_max_column * minimap_char_width).floor();
        let minimap_width = js_min(
            minimap_max_width,
            js_max(
                0.0,
                (((remaining_width - vertical_scrollbar_width - 2.0) * minimap_char_width)
                    / (typical_halfwidth_character_width + minimap_char_width))
                    .floor(),
            ) + MINIMAP_GUTTER_WIDTH,
        );

        let mut minimap_canvas_inner_width = (pixel_ratio * minimap_width).floor();
        let minimap_canvas_outer_width = minimap_canvas_inner_width / pixel_ratio;
        minimap_canvas_inner_width = (minimap_canvas_inner_width * minimap_width_multiplier).floor();

        let render_minimap = if minimap_render_characters { RenderMinimap::Text } else { RenderMinimap::Blocks };
        let minimap_left = if minimap_side == MinimapSide::Left { 0.0 } else { outer_width - minimap_width - vertical_scrollbar_width };

        EditorMinimapLayoutInfo {
            render_minimap,
            minimap_left,
            minimap_width,
            minimap_height_is_editor_height,
            minimap_is_sampling,
            minimap_scale,
            minimap_line_height,
            minimap_canvas_inner_width,
            minimap_canvas_inner_height,
            minimap_canvas_outer_width,
            minimap_canvas_outer_height,
        }
    }

    /// `computeLayout`.
    pub fn compute_layout(options: &ComputedEditorOptions, env: &EditorLayoutInfoComputerEnv) -> EditorLayoutInfo {
        let outer_width = to_int32(env.outer_width);
        let outer_height = to_int32(env.outer_height);
        let line_height = to_int32(env.line_height);
        let line_numbers_digit_count = to_int32(env.line_numbers_digit_count);
        let typical_halfwidth_character_width = env.typical_halfwidth_character_width;
        let max_digit_width = env.max_digit_width;
        let pixel_ratio = env.pixel_ratio;
        let view_line_count = env.view_line_count;

        let word_wrap_override_2 = *options.get(WORD_WRAP_OVERRIDE_2);
        let word_wrap_override_1 =
            if word_wrap_override_2 == WordWrapOverride::Inherit { *options.get(WORD_WRAP_OVERRIDE_1) } else { word_wrap_override_2 };
        let word_wrap = match word_wrap_override_1 {
            WordWrapOverride::Inherit => *options.get(WORD_WRAP),
            WordWrapOverride::On => WordWrap::On,
            WordWrapOverride::Off => WordWrap::Off,
        };

        let word_wrap_column = *options.get(WORD_WRAP_COLUMN);
        let word_wrap_indicator = *options.get(WORD_WRAP_INDICATOR);
        let is_dominated_by_long_lines = env.is_dominated_by_long_lines;

        let show_glyph_margin = *options.get(GLYPH_MARGIN);
        let show_line_numbers = options.get(LINE_NUMBERS).render_type != RenderLineNumbersType::Off;
        let line_numbers_min_chars = *options.get(LINE_NUMBERS_MIN_CHARS);
        let scroll_beyond_last_line = *options.get(SCROLL_BEYOND_LAST_LINE);
        let padding = options.get(PADDING);
        let minimap = options.get(MINIMAP);

        let scrollbar = options.get(SCROLLBAR);
        let vertical_scrollbar_width = scrollbar.vertical_scrollbar_size;
        let vertical_scrollbar_has_arrows = scrollbar.vertical_has_arrows;
        let scrollbar_arrow_size = scrollbar.arrow_size;
        let horizontal_scrollbar_height = scrollbar.horizontal_scrollbar_size;

        let folding = *options.get(FOLDING);
        let show_folding_decoration = *options.get(SHOW_FOLDING_CONTROLS) != ShowFoldingControls::Never;

        let mut line_decorations_width = *options.get(LINE_DECORATIONS_WIDTH);
        if folding && show_folding_decoration {
            line_decorations_width += 16.0;
        }

        let mut line_numbers_width = 0.0;
        if show_line_numbers {
            let digit_count = js_max(line_numbers_digit_count, line_numbers_min_chars);
            line_numbers_width = (digit_count * max_digit_width).round();
        }

        let mut glyph_margin_width = 0.0;
        if show_glyph_margin {
            glyph_margin_width = line_height * env.glyph_margin_decoration_lane_count;
        }

        let mut glyph_margin_left = 0.0;
        let mut line_numbers_left = glyph_margin_left + glyph_margin_width;
        let mut decorations_left = line_numbers_left + line_numbers_width;
        let mut content_left = decorations_left + line_decorations_width;

        let remaining_width = outer_width - glyph_margin_width - line_numbers_width - line_decorations_width;

        let mut is_word_wrap_minified = false;
        let mut is_viewport_wrapping = false;
        let mut wrapping_column = -1.0;

        if *options.get(ACCESSIBILITY_SUPPORT) == AccessibilitySupport::Enabled
            && word_wrap_override_1 == WordWrapOverride::Inherit
            && is_dominated_by_long_lines
        {
            // Força a quebra pela largura da viewport se o modelo é dominado por linhas longas.
            is_word_wrap_minified = true;
            is_viewport_wrapping = true;
        } else if word_wrap == WordWrap::On || word_wrap == WordWrap::Bounded {
            is_viewport_wrapping = true;
        } else if word_wrap == WordWrap::WordWrapColumn {
            wrapping_column = word_wrap_column;
        }

        let minimap_input = MinimapLayoutInput {
            outer_width,
            outer_height,
            line_height,
            typical_halfwidth_character_width,
            pixel_ratio,
            scroll_beyond_last_line,
            padding_top: padding.top,
            padding_bottom: padding.bottom,
            minimap: minimap.clone(),
            vertical_scrollbar_width,
            view_line_count,
            remaining_width,
            is_viewport_wrapping,
        };
        let memory = env.memory.clone().unwrap_or_else(|| Rc::new(RefCell::new(ComputeOptionsMemory::new())));
        let minimap_layout = Self::compute_minimap_layout(&minimap_input, &mut memory.borrow_mut());

        if minimap_layout.render_minimap != RenderMinimap::None && minimap_layout.minimap_left == 0.0 {
            // O minimapa é desenhado à esquerda, então tudo vai para a direita.
            glyph_margin_left += minimap_layout.minimap_width;
            line_numbers_left += minimap_layout.minimap_width;
            decorations_left += minimap_layout.minimap_width;
            content_left += minimap_layout.minimap_width;
        }
        let content_width = remaining_width - minimap_layout.minimap_width;

        // (deixando 2px para o cursor ter espaço depois do último caractere)
        let viewport_column = js_max(1.0, ((content_width - vertical_scrollbar_width - 2.0) / typical_halfwidth_character_width).floor());

        let vertical_arrow_size = if vertical_scrollbar_has_arrows { scrollbar_arrow_size } else { 0.0 };

        if is_viewport_wrapping {
            // Calcula o `wrappingColumn` de fato (deixando a coluna da direita para o indicador de quebra, para
            // ele não sobrepor o texto quebrado nem ficar embaixo da barra de rolagem vertical).
            wrapping_column = js_max(1.0, viewport_column - if word_wrap_indicator { 1.0 } else { 0.0 });
            if word_wrap == WordWrap::Bounded {
                wrapping_column = js_min(wrapping_column, word_wrap_column);
            }
        }

        EditorLayoutInfo {
            width: outer_width,
            height: outer_height,

            glyph_margin_left,
            glyph_margin_width,
            glyph_margin_decoration_lane_count: env.glyph_margin_decoration_lane_count,

            line_numbers_left,
            line_numbers_width,

            decorations_left,
            decorations_width: line_decorations_width,

            content_left,
            content_width,

            minimap: minimap_layout,

            viewport_column,

            is_word_wrap_minified,
            is_viewport_wrapping,
            wrapping_column,

            vertical_scrollbar_width,
            horizontal_scrollbar_height,

            overview_ruler: OverviewRulerPosition {
                top: vertical_arrow_size,
                width: vertical_scrollbar_width,
                height: outer_height - 2.0 * vertical_arrow_size,
                right: 0.0,
            },
        }
    }
}

impl Default for EditorLayoutInfoComputer {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorOptionImpl for EditorLayoutInfoComputer {
    type Value = EditorLayoutInfo;

    fn id(&self) -> EditorOption {
        self.inner.id()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn default_value(&self) -> EditorLayoutInfo {
        self.inner.default_value()
    }

    fn validate(&self, input: &Dynamic) -> EditorLayoutInfo {
        self.inner.validate(input)
    }

    fn compute(&self, env: &EnvironmentalOptions, options: &ComputedEditorOptions, value: &EditorLayoutInfo) -> EditorLayoutInfo {
        self.inner.compute(env, options, value)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::editor_options::layout_info::LAYOUT_INFO;
    use super::*;

    #[test]
    fn identity_and_default() {
        let option = EditorLayoutInfoComputer::new();
        assert_eq!(option.id(), EditorOption::LayoutInfo);
        assert_eq!(option.name(), "_never_");
        assert_eq!(option.default_value(), default_layout_info());
        assert_eq!(LAYOUT_INFO.id(), EditorOption::LayoutInfo);
        assert_eq!(option.default_value().wrapping_column, -1.0);
        assert_eq!(option.default_value().minimap.minimap_scale, 1.0);
    }

    #[test]
    fn contained_line_count_follows_the_formulas() {
        let result = EditorLayoutInfoComputer::compute_contained_minimap_line_count(&ContainedMinimapLineCountInput {
            view_line_count: 100.0,
            scroll_beyond_last_line: false,
            padding_top: 40.0,
            padding_bottom: 20.0,
            height: 200.0,
            line_height: 20.0,
            pixel_ratio: 1.0,
        });
        assert_eq!(result.typical_viewport_line_count, 10.0);
        assert_eq!(result.extra_lines_before_first_line, 2.0);
        assert_eq!(result.extra_lines_beyond_last_line, 1.0);
        assert_eq!(result.desired_ratio, 103.0 / 200.0);
        assert_eq!(result.minimap_line_count, (100.0_f64 / (103.0 / 200.0)).floor());
        let beyond = EditorLayoutInfoComputer::compute_contained_minimap_line_count(&ContainedMinimapLineCountInput {
            view_line_count: 100.0,
            scroll_beyond_last_line: true,
            padding_top: 0.0,
            padding_bottom: 0.0,
            height: 200.0,
            line_height: 20.0,
            pixel_ratio: 1.0,
        });
        assert_eq!(beyond.extra_lines_beyond_last_line, 9.0);
    }
}
