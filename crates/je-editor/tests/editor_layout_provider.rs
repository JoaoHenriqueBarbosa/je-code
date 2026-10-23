//! Porte de `src/vs/editor/test/browser/config/editorLayoutProvider.test.ts` (suíte
//! `Editor ViewLayout - EditorLayoutProvider`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `new ComputedEditorOptions()` seguido de `_write(...)` vira um vetor de valores com um `()` em cada
//!   opção que o teste não escreve, e as opções escritas entram pela chave tipada (`write`).
//! - O original não escreve `scrollBeyondLastLine` nem `showFoldingControls` (o `get` devolve `undefined`).
//!   O porte tipado precisa de um valor: `scrollBeyondLastLine` fica `false` (o `undefined` é falso) e
//!   `showFoldingControls` fica `Mouseover` (o padrão), que não importa porque `folding` é `false`.
//! - `accessibilitySupport` escrito como `'auto'` vira `AccessibilitySupport::Unknown`, o valor interno de
//!   `auto` (o computador só reage a `Enabled`).
//! - `EditorOptions.scrollbar.defaultValue` é `EditorScrollbar::new().default_value()`.
//! - `assert.deepStrictEqual` sobre o `EditorLayoutInfo` inteiro vira `assert_eq!` sobre o struct inteiro.
//! - `input.maxLineNumber || Math.pow(10, input.lineNumbersDigitCount) - 1` vira `Option::filter` (o `||` do
//!   JavaScript trata `0` como ausente) seguido de `unwrap_or`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::config::editor_options::accessibility_support::ACCESSIBILITY_SUPPORT;
use je_editor::config::editor_options::layout_info::{
    EditorLayoutInfo, EditorMinimapLayoutInfo, OverviewRulerPosition, RenderMinimap,
};
use je_editor::config::editor_options::layout_info_computer::{EditorLayoutInfoComputer, EditorLayoutInfoComputerEnv};
use je_editor::config::editor_options::line_decorations_width::LINE_DECORATIONS_WIDTH;
use je_editor::config::editor_options::line_numbers::{
    InternalEditorRenderLineNumbersOptions, RenderLineNumbersType, LINE_NUMBERS,
};
use je_editor::config::editor_options::minimap::{
    EditorMinimapOptions, MinimapAutohide, MinimapShowSlider, MinimapSide, MinimapSize, MINIMAP,
};
use je_editor::config::editor_options::padding::{InternalEditorPaddingOptions, PADDING};
use je_editor::config::editor_options::scrollbar::{EditorScrollbar, SCROLLBAR};
use je_editor::config::editor_options::simple_keys::{
    ShowFoldingControls, WordWrap, WordWrapOverride, FOLDING, GLYPH_MARGIN, LINE_NUMBERS_MIN_CHARS,
    SCROLL_BEYOND_LAST_LINE, SHOW_FOLDING_CONTROLS, WORD_WRAP, WORD_WRAP_COLUMN, WORD_WRAP_INDICATOR,
    WORD_WRAP_OVERRIDE_1, WORD_WRAP_OVERRIDE_2,
};
use je_editor::config::editor_options::{
    AccessibilitySupport, ComputedEditorOptions, EditorOption, EditorOptionImpl, Opt,
};

/// `IEditorLayoutProviderOpts`. Os campos opcionais do original são `Option`.
struct EditorLayoutProviderOpts {
    outer_width: f64,
    outer_height: f64,

    show_glyph_margin: bool,
    line_height: f64,

    show_line_numbers: bool,
    line_numbers_min_chars: f64,
    line_numbers_digit_count: f64,
    max_line_number: Option<f64>,

    line_decorations_width: f64,

    typical_halfwidth_character_width: f64,
    max_digit_width: f64,

    vertical_scrollbar_width: f64,
    vertical_scrollbar_has_arrows: bool,
    scrollbar_arrow_size: f64,
    horizontal_scrollbar_height: f64,

    minimap: bool,
    minimap_side: MinimapSide,
    minimap_render_characters: bool,
    minimap_max_column: f64,
    minimap_size: Option<MinimapSize>,
    pixel_ratio: f64,

    /// Padrão `'off'`, ou seja, um layout que não quebra linha.
    word_wrap: Option<WordWrap>,
    word_wrap_column: Option<f64>,
    word_wrap_indicator: Option<bool>,
}

/// Os campos opcionais ausentes (`maxLineNumber`, `minimapSize` e os três de quebra de linha), para
/// `..unset_optionals()` nos casos em que o original não os informa.
fn unset_optionals() -> EditorLayoutProviderOpts {
    EditorLayoutProviderOpts {
        outer_width: 0.0,
        outer_height: 0.0,
        show_glyph_margin: false,
        line_height: 0.0,
        show_line_numbers: false,
        line_numbers_min_chars: 0.0,
        line_numbers_digit_count: 0.0,
        max_line_number: None,
        line_decorations_width: 0.0,
        typical_halfwidth_character_width: 0.0,
        max_digit_width: 0.0,
        vertical_scrollbar_width: 0.0,
        vertical_scrollbar_has_arrows: false,
        scrollbar_arrow_size: 0.0,
        horizontal_scrollbar_height: 0.0,
        minimap: false,
        minimap_side: MinimapSide::Right,
        minimap_render_characters: false,
        minimap_max_column: 0.0,
        minimap_size: None,
        pixel_ratio: 0.0,
        word_wrap: None,
        word_wrap_column: None,
        word_wrap_indicator: None,
    }
}

/// `options._write(option, value)`: escreve o valor na posição da opção.
fn write<T: 'static>(values: &mut [Rc<dyn Any>], key: Opt<T>, value: T) {
    values[key.id() as usize] = Rc::new(value);
}

fn compute_layout_info(input: &EditorLayoutProviderOpts) -> EditorLayoutInfo {
    let mut values: Vec<Rc<dyn Any>> = EditorOption::ALL.iter().map(|_| Rc::new(()) as Rc<dyn Any>).collect();
    write(&mut values, GLYPH_MARGIN, input.show_glyph_margin);
    write(&mut values, LINE_NUMBERS_MIN_CHARS, input.line_numbers_min_chars);
    write(&mut values, LINE_DECORATIONS_WIDTH, input.line_decorations_width);
    write(&mut values, FOLDING, false);
    write(&mut values, PADDING, InternalEditorPaddingOptions { top: 0.0, bottom: 0.0 });
    let minimap_options = EditorMinimapOptions {
        enabled: input.minimap,
        autohide: MinimapAutohide::None,
        size: input.minimap_size.unwrap_or(MinimapSize::Proportional),
        side: input.minimap_side,
        render_characters: input.minimap_render_characters,
        max_column: input.minimap_max_column,
        show_slider: MinimapShowSlider::Mouseover,
        scale: 1.0,
        show_region_section_headers: true,
        show_mark_section_headers: true,
        section_header_font_size: 9.0,
        section_header_letter_spacing: 1.0,
        mark_section_header_regex: r"\bMARK:\s*(?<separator>-?)\s*(?<label>.*)$".to_string(),
    };
    write(&mut values, MINIMAP, minimap_options);
    let default_scrollbar = EditorScrollbar::new().default_value();
    let mut scrollbar_options = default_scrollbar;
    scrollbar_options.arrow_size = input.scrollbar_arrow_size;
    scrollbar_options.vertical_has_arrows = input.vertical_scrollbar_has_arrows;
    scrollbar_options.horizontal_has_arrows = false;
    scrollbar_options.always_consume_mouse_wheel = true;
    scrollbar_options.horizontal_scrollbar_size = input.horizontal_scrollbar_height;
    scrollbar_options.vertical_scrollbar_size = input.vertical_scrollbar_width;
    scrollbar_options.ignore_horizontal_scrollbar_in_content_height = false;
    write(&mut values, SCROLLBAR, scrollbar_options);
    let line_numbers_options = InternalEditorRenderLineNumbersOptions {
        render_type: if input.show_line_numbers { RenderLineNumbersType::On } else { RenderLineNumbersType::Off },
        render_fn: None,
    };
    write(&mut values, LINE_NUMBERS, line_numbers_options);

    write(&mut values, WORD_WRAP, input.word_wrap.unwrap_or(WordWrap::Off));
    write(&mut values, WORD_WRAP_COLUMN, input.word_wrap_column.unwrap_or(80.0));
    write(&mut values, WORD_WRAP_INDICATOR, input.word_wrap_indicator.unwrap_or(false));
    write(&mut values, WORD_WRAP_OVERRIDE_1, WordWrapOverride::Inherit);
    write(&mut values, WORD_WRAP_OVERRIDE_2, WordWrapOverride::Inherit);
    write(&mut values, ACCESSIBILITY_SUPPORT, AccessibilitySupport::Unknown);
    // Não escritas no original (o `get` devolve `undefined`); ver as notas no começo do arquivo.
    write(&mut values, SCROLL_BEYOND_LAST_LINE, false);
    write(&mut values, SHOW_FOLDING_CONTROLS, ShowFoldingControls::Mouseover);
    let options = ComputedEditorOptions::new(values);

    EditorLayoutInfoComputer::compute_layout(
        &options,
        &EditorLayoutInfoComputerEnv {
            memory: None,
            outer_width: input.outer_width,
            outer_height: input.outer_height,
            is_dominated_by_long_lines: false,
            line_height: input.line_height,
            view_line_count: input
                .max_line_number
                .filter(|n| *n != 0.0)
                .unwrap_or_else(|| 10f64.powf(input.line_numbers_digit_count) - 1.0),
            line_numbers_digit_count: input.line_numbers_digit_count,
            typical_halfwidth_character_width: input.typical_halfwidth_character_width,
            max_digit_width: input.max_digit_width,
            pixel_ratio: input.pixel_ratio,
            glyph_margin_decoration_lane_count: 1.0,
        },
    )
}

fn do_test(input: EditorLayoutProviderOpts, expected: EditorLayoutInfo) {
    assert_eq!(compute_layout_info(&input), expected);
}

/// O `minimap` esperado quando o minimapa está desligado (`RenderMinimap.None`), numa altura `height`.
fn minimap_off(height: f64) -> EditorMinimapLayoutInfo {
    EditorMinimapLayoutInfo {
        render_minimap: RenderMinimap::None,
        minimap_left: 0.0,
        minimap_width: 0.0,
        minimap_height_is_editor_height: false,
        minimap_is_sampling: false,
        minimap_scale: 1.0,
        minimap_line_height: 1.0,
        minimap_canvas_inner_width: 0.0,
        minimap_canvas_inner_height: height,
        minimap_canvas_outer_width: 0.0,
        minimap_canvas_outer_height: height,
    }
}

/// O `overviewRuler` esperado (`right` é sempre 0).
fn ruler(top: f64, width: f64, height: f64) -> OverviewRulerPosition {
    OverviewRulerPosition { top, width, height, right: 0.0 }
}

#[test]
fn editor_layout_provider_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 990.0,

            minimap: minimap_off(800.0),

            viewport_column: 98.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_1_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 11.0,
            horizontal_scrollbar_height: 12.0,
            scrollbar_arrow_size: 13.0,
            vertical_scrollbar_has_arrows: true,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 990.0,

            minimap: minimap_off(800.0),

            viewport_column: 97.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 11.0,
            horizontal_scrollbar_height: 12.0,

            overview_ruler: ruler(13.0, 11.0, 800.0 - 2.0 * 13.0),
        },
    );
}

#[test]
fn editor_layout_provider_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 900.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 900.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 890.0,

            minimap: minimap_off(800.0),

            viewport_column: 88.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 900.0,
            outer_height: 900.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 900.0,
            height: 900.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 890.0,

            minimap: minimap_off(900.0),

            viewport_column: 88.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 900.0),
        },
    );
}

#[test]
fn editor_layout_provider_4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 900.0,
            outer_height: 900.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 5.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 900.0,
            height: 900.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 890.0,

            minimap: minimap_off(900.0),

            viewport_column: 88.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 900.0),
        },
    );
}

#[test]
fn editor_layout_provider_5() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 900.0,
            outer_height: 900.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: true,
            line_numbers_min_chars: 5.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 900.0,
            height: 900.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 50.0,

            decorations_left: 50.0,
            decorations_width: 10.0,

            content_left: 60.0,
            content_width: 840.0,

            minimap: minimap_off(900.0),

            viewport_column: 83.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 900.0),
        },
    );
}

#[test]
fn editor_layout_provider_6() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 900.0,
            outer_height: 900.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: true,
            line_numbers_min_chars: 5.0,
            line_numbers_digit_count: 5.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 900.0,
            height: 900.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 50.0,

            decorations_left: 50.0,
            decorations_width: 10.0,

            content_left: 60.0,
            content_width: 840.0,

            minimap: minimap_off(900.0),

            viewport_column: 83.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 900.0),
        },
    );
}

#[test]
fn editor_layout_provider_7() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 900.0,
            outer_height: 900.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: true,
            line_numbers_min_chars: 5.0,
            line_numbers_digit_count: 6.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 900.0,
            height: 900.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 60.0,

            decorations_left: 60.0,
            decorations_width: 10.0,

            content_left: 70.0,
            content_width: 830.0,

            minimap: minimap_off(900.0),

            viewport_column: 82.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 900.0),
        },
    );
}

#[test]
fn editor_layout_provider_8() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 900.0,
            outer_height: 900.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: true,
            line_numbers_min_chars: 5.0,
            line_numbers_digit_count: 6.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 5.0,
            max_digit_width: 5.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 900.0,
            height: 900.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 30.0,

            decorations_left: 30.0,
            decorations_width: 10.0,

            content_left: 40.0,
            content_width: 860.0,

            minimap: minimap_off(900.0),

            viewport_column: 171.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 900.0),
        },
    );
}

#[test]
fn editor_layout_provider_8_rounds_floats() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 900.0,
            outer_height: 900.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: true,
            line_numbers_min_chars: 5.0,
            line_numbers_digit_count: 6.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 5.05,
            max_digit_width: 5.05,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 900.0,
            height: 900.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 30.0,

            decorations_left: 30.0,
            decorations_width: 10.0,

            content_left: 40.0,
            content_width: 860.0,

            minimap: minimap_off(900.0),

            viewport_column: 169.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 900.0),
        },
    );
}

#[test]
fn editor_layout_provider_9_render_minimap() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 893.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 903.0,
                minimap_width: 97.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 1.0,
                minimap_line_height: 2.0,
                minimap_canvas_inner_width: 97.0,
                minimap_canvas_inner_height: 800.0,
                minimap_canvas_outer_width: 97.0,
                minimap_canvas_outer_height: 800.0,
            },

            viewport_column: 89.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_9_render_minimap_with_pixel_ratio_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 2.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 893.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 903.0,
                minimap_width: 97.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 2.0,
                minimap_line_height: 4.0,
                minimap_canvas_inner_width: 194.0,
                minimap_canvas_inner_height: 1600.0,
                minimap_canvas_outer_width: 97.0,
                minimap_canvas_outer_height: 800.0,
            },

            viewport_column: 89.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_9_render_minimap_with_pixel_ratio_4() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 4.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 935.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 945.0,
                minimap_width: 55.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 2.0,
                minimap_line_height: 4.0,
                minimap_canvas_inner_width: 220.0,
                minimap_canvas_inner_height: 3200.0,
                minimap_canvas_outer_width: 55.0,
                minimap_canvas_outer_height: 800.0,
            },

            viewport_column: 93.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_10_render_minimap_to_left() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Left,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 4.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 55.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 55.0,
            line_numbers_width: 0.0,

            decorations_left: 55.0,
            decorations_width: 10.0,

            content_left: 65.0,
            content_width: 935.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 0.0,
                minimap_width: 55.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 2.0,
                minimap_line_height: 4.0,
                minimap_canvas_inner_width: 220.0,
                minimap_canvas_inner_height: 3200.0,
                minimap_canvas_outer_width: 55.0,
                minimap_canvas_outer_height: 800.0,
            },

            viewport_column: 93.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_11_minimap_mode_cover_without_sampling() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 3.0,
            max_line_number: Some(120.0),
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            minimap_size: Some(MinimapSize::Fill),
            pixel_ratio: 2.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 893.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 903.0,
                minimap_width: 97.0,
                minimap_height_is_editor_height: true,
                minimap_is_sampling: false,
                minimap_scale: 3.0,
                minimap_line_height: 13.0,
                minimap_canvas_inner_width: 291.0,
                minimap_canvas_inner_height: 1560.0,
                minimap_canvas_outer_width: 97.0,
                minimap_canvas_outer_height: 800.0,
            },

            viewport_column: 89.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_12_minimap_mode_cover_with_sampling() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 4.0,
            max_line_number: Some(2500.0),
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            minimap_size: Some(MinimapSize::Fill),
            pixel_ratio: 2.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 935.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 945.0,
                minimap_width: 55.0,
                minimap_height_is_editor_height: true,
                minimap_is_sampling: true,
                minimap_scale: 1.0,
                minimap_line_height: 1.0,
                minimap_canvas_inner_width: 110.0,
                minimap_canvas_inner_height: 1600.0,
                minimap_canvas_outer_width: 55.0,
                minimap_canvas_outer_height: 800.0,
            },

            viewport_column: 93.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_13_minimap_mode_contain_without_sampling() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 3.0,
            max_line_number: Some(120.0),
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            minimap_size: Some(MinimapSize::Fit),
            pixel_ratio: 2.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 893.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 903.0,
                minimap_width: 97.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 2.0,
                minimap_line_height: 4.0,
                minimap_canvas_inner_width: 194.0,
                minimap_canvas_inner_height: 1600.0,
                minimap_canvas_outer_width: 97.0,
                minimap_canvas_outer_height: 800.0,
            },

            viewport_column: 89.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn editor_layout_provider_14_minimap_mode_contain_with_sampling() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 4.0,
            max_line_number: Some(2500.0),
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            scrollbar_arrow_size: 0.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            minimap_size: Some(MinimapSize::Fit),
            pixel_ratio: 2.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1000.0,
            height: 800.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 0.0,
            line_numbers_width: 0.0,

            decorations_left: 0.0,
            decorations_width: 10.0,

            content_left: 10.0,
            content_width: 935.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 945.0,
                minimap_width: 55.0,
                minimap_height_is_editor_height: true,
                minimap_is_sampling: true,
                minimap_scale: 1.0,
                minimap_line_height: 1.0,
                minimap_canvas_inner_width: 110.0,
                minimap_canvas_inner_height: 1600.0,
                minimap_canvas_outer_width: 55.0,
                minimap_canvas_outer_height: 800.0,
            },

            viewport_column: 93.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,

            overview_ruler: ruler(0.0, 0.0, 800.0),
        },
    );
}

#[test]
fn issue_31312_when_wrapping_leave_2px_for_the_cursor() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    do_test(
        EditorLayoutProviderOpts {
            outer_width: 1201.0,
            outer_height: 422.0,
            show_glyph_margin: true,
            line_height: 30.0,
            show_line_numbers: true,
            line_numbers_min_chars: 3.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 26.0,
            typical_halfwidth_character_width: 12.04296875,
            max_digit_width: 12.04296875,
            vertical_scrollbar_width: 14.0,
            horizontal_scrollbar_height: 10.0,
            scrollbar_arrow_size: 11.0,
            vertical_scrollbar_has_arrows: false,
            minimap: true,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 120.0,
            pixel_ratio: 2.0,
            ..unset_optionals()
        },
        EditorLayoutInfo {
            width: 1201.0,
            height: 422.0,

            glyph_margin_left: 0.0,
            glyph_margin_width: 30.0,
            glyph_margin_decoration_lane_count: 1.0,

            line_numbers_left: 30.0,
            line_numbers_width: 36.0,

            decorations_left: 66.0,
            decorations_width: 26.0,

            content_left: 92.0,
            content_width: 1018.0,

            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::Text,
                minimap_left: 1096.0,
                minimap_width: 91.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 2.0,
                minimap_line_height: 4.0,
                minimap_canvas_inner_width: 182.0,
                minimap_canvas_inner_height: 844.0,
                minimap_canvas_outer_width: 91.0,
                minimap_canvas_outer_height: 422.0,
            },

            viewport_column: 83.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,

            vertical_scrollbar_width: 14.0,
            horizontal_scrollbar_height: 10.0,

            overview_ruler: ruler(0.0, 14.0, 422.0),
        },
    );
}

#[test]
fn leaves_the_rightmost_column_for_the_word_wrap_indicator_when_wrapping_to_the_viewport() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // 990px de conteúdo a 10px por caractere cabem 98 colunas, depois de tirar os 2px reservados ao cursor.
    // O indicador ocupa a coluna mais à direita, então o texto quebrado precisa parar uma coluna antes, em
    // vez de sobrepor o glifo.
    let wrapping_of = |word_wrap: WordWrap, word_wrap_indicator: bool, word_wrap_column: f64| {
        let layout_info = compute_layout_info(&EditorLayoutProviderOpts {
            outer_width: 1000.0,
            outer_height: 800.0,
            show_glyph_margin: false,
            line_height: 16.0,
            show_line_numbers: false,
            line_numbers_min_chars: 0.0,
            line_numbers_digit_count: 1.0,
            line_decorations_width: 10.0,
            typical_halfwidth_character_width: 10.0,
            max_digit_width: 10.0,
            vertical_scrollbar_width: 0.0,
            vertical_scrollbar_has_arrows: false,
            scrollbar_arrow_size: 0.0,
            horizontal_scrollbar_height: 0.0,
            minimap: false,
            minimap_side: MinimapSide::Right,
            minimap_render_characters: true,
            minimap_max_column: 150.0,
            pixel_ratio: 1.0,
            word_wrap: Some(word_wrap),
            word_wrap_column: Some(word_wrap_column),
            word_wrap_indicator: Some(word_wrap_indicator),
            ..unset_optionals()
        });
        // `viewportColumn` continua descrevendo quantas colunas a viewport tem, para o minimapa, que é
        // dimensionado pela mesma largura, não sofrer efeito do indicador.
        (layout_info.viewport_column, layout_info.wrapping_column)
    };

    let actual = [
        ("off", wrapping_of(WordWrap::Off, true, 40.0)),
        ("on", wrapping_of(WordWrap::On, false, 40.0)),
        ("onWithIndicator", wrapping_of(WordWrap::On, true, 40.0)),
        // Os dois limitam a coluna de quebra bem abaixo da viewport, então já sobra largura para o glifo.
        ("bounded", wrapping_of(WordWrap::Bounded, false, 40.0)),
        ("boundedWithIndicator", wrapping_of(WordWrap::Bounded, true, 40.0)),
        ("wordWrapColumnWithIndicator", wrapping_of(WordWrap::WordWrapColumn, true, 40.0)),
        ("wideWordWrapColumnWithIndicator", wrapping_of(WordWrap::WordWrapColumn, true, 120.0)),
    ];
    let expected = [
        ("off", (98.0, -1.0)),
        ("on", (98.0, 98.0)),
        ("onWithIndicator", (98.0, 97.0)),
        ("bounded", (98.0, 40.0)),
        ("boundedWithIndicator", (98.0, 40.0)),
        ("wordWrapColumnWithIndicator", (98.0, 40.0)),
        ("wideWordWrapColumnWithIndicator", (98.0, 120.0)),
    ];
    assert_eq!(actual, expected);
}
