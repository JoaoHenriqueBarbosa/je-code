//! Porte de `src/vs/editor/contrib/inlineCompletions/test/browser/longDistanceWidgetPlacement.test.ts` (suíte `WidgetPlacementContext`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - As suítes aninhadas viram prefixos no nome do teste (`constructor_available_space_sizes_...`).
//! - Os números do JavaScript são `f64`; `endOfLinePadding` é um fecho `i32 -> f64`.
//! - `createLineRangeInfo` devolve um `ContinuousLineSizes`, que o contexto recebe por valor (`clone` onde o
//!   original reaproveita o mesmo objeto, no caso `prefers closer lines`).
//! - `getWidgetVerticalOutline` devolve `Option` (o original devolveria um intervalo `NaN` fora da faixa);
//!   os casos usam `unwrap` porque a linha sempre está dentro da faixa.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::core_2d::size::Size2D;
use je_editor::inline_completions::long_distance_widget_placement::{ContinuousLineSizes, WidgetLayoutConstants, WidgetPlacementContext};
use je_editor::line_range::LineRange;

fn create_line_range_info(start_line: i32, sizes: Vec<Size2D>, top: f64) -> ContinuousLineSizes {
    ContinuousLineSizes { line_range: LineRange::of_length(start_line, sizes.len() as i32), top, sizes }
}

const DEFAULT_LAYOUT_CONSTANTS: WidgetLayoutConstants = WidgetLayoutConstants {
    preview_editor_margin: 5.0,
    widget_padding: 2.0,
    widget_border: 1.0,
    lower_bar_height: 10.0,
    min_widget_width: 50.0,
};

fn sz(width: f64, height: f64) -> Size2D {
    Size2D::new(width, height)
}

// constructor - availableSpaceSizes computation

#[test]
fn constructor_available_space_sizes_computes_correctly_with_no_padding() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0), sz(150.0, 20.0), sz(80.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);
    let editor_true_content_width = 500.0;

    let context = WidgetPlacementContext::new(line_range_info, editor_true_content_width, |_| 0.0);

    assert_eq!(context.available_space_sizes.len(), 3);
    assert_eq!(context.available_space_sizes[0].width, 400.0); // 500 - 100
    assert_eq!(context.available_space_sizes[1].width, 350.0); // 500 - 150
    assert_eq!(context.available_space_sizes[2].width, 420.0); // 500 - 80
}

#[test]
fn constructor_available_space_sizes_computes_with_end_of_line_padding() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0), sz(150.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);
    let editor_true_content_width = 500.0;

    let context = WidgetPlacementContext::new(line_range_info, editor_true_content_width, |line_number| f64::from(line_number) * 10.0);

    assert_eq!(context.available_space_sizes[0].width, 390.0); // 500 - 100 - 10
    assert_eq!(context.available_space_sizes[1].width, 330.0); // 500 - 150 - 20
}

#[test]
fn constructor_available_space_sizes_width_is_never_negative() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(600.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);
    let editor_true_content_width = 500.0;

    let context = WidgetPlacementContext::new(line_range_info, editor_true_content_width, |_| 0.0);

    assert_eq!(context.available_space_sizes[0].width, 0.0);
}

#[test]
fn constructor_available_space_sizes_preserves_heights() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 25.0), sz(100.0, 30.0), sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);
    let editor_true_content_width = 500.0;

    let context = WidgetPlacementContext::new(line_range_info, editor_true_content_width, |_| 0.0);

    assert_eq!(context.available_space_sizes[0].height, 25.0);
    assert_eq!(context.available_space_sizes[1].height, 30.0);
    assert_eq!(context.available_space_sizes[2].height, 20.0);
}

// constructor - prefix sums computation

#[test]
fn constructor_prefix_sums_computes_height_prefix_sums_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0), sz(100.0, 30.0), sz(100.0, 25.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);

    assert_eq!(context.available_space_height_prefix_sums, vec![0.0, 20.0, 50.0, 75.0]);
}

#[test]
fn constructor_prefix_sums_start_with_0_and_have_length_sizes_plus_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 10.0), sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);

    assert_eq!(context.available_space_height_prefix_sums[0], 0.0);
    assert_eq!(context.available_space_height_prefix_sums.len(), 3);
}

// constructor - transposed sizes

#[test]
fn constructor_transposed_sizes_transposes_width_and_height_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0), sz(150.0, 30.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);

    // Transposto: a largura vira altura e vice-versa.
    // As larguras disponíveis são 400 e 350, as alturas são 20 e 30.
    assert_eq!(context.available_space_sizes_transposed[0].width, 20.0);
    assert_eq!(context.available_space_sizes_transposed[0].height, 400.0);
    assert_eq!(context.available_space_sizes_transposed[1].width, 30.0);
    assert_eq!(context.available_space_sizes_transposed[1].height, 350.0);
}

// getWidgetVerticalOutline

#[test]
fn get_widget_vertical_outline_computes_for_first_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0), sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 100.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);
    let outline = context.get_widget_vertical_outline(1, 50.0, DEFAULT_LAYOUT_CONSTANTS).unwrap();

    // previewEditorMargin + widgetPadding + widgetBorder = 5 + 2 + 1 = 8
    // editorRange = [100, 150)
    // verticalWidgetRange = [100 - 8, 150 + 8 + 10) = [92, 168)
    assert_eq!(outline.start, 92);
    assert_eq!(outline.end_exclusive, 168);
}

#[test]
fn get_widget_vertical_outline_computes_for_second_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0), sz(100.0, 25.0)];
    let line_range_info = create_line_range_info(1, sizes, 100.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);
    let outline = context.get_widget_vertical_outline(2, 50.0, DEFAULT_LAYOUT_CONSTANTS).unwrap();

    // A linha 2 está no índice 1, prefixSum[1] = 20
    // top = 100 + 20 = 120
    // editorRange = [120, 170)
    // margin = 8, lowerBarHeight = 10
    // verticalWidgetRange = [120 - 8, 170 + 8 + 10) = [112, 188)
    assert_eq!(outline.start, 112);
    assert_eq!(outline.end_exclusive, 188);
}

#[test]
fn get_widget_vertical_outline_works_with_zero_margins() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);
    let zero_constants = WidgetLayoutConstants {
        preview_editor_margin: 0.0,
        widget_padding: 0.0,
        widget_border: 0.0,
        lower_bar_height: 0.0,
        min_widget_width: 50.0,
    };

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);
    let outline = context.get_widget_vertical_outline(1, 50.0, zero_constants).unwrap();

    assert_eq!(outline.start, 0);
    assert_eq!(outline.end_exclusive, 50);
}

// tryFindWidgetOutline

#[test]
fn try_find_widget_outline_returns_undefined_when_no_line_has_enough_width() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Todas as linhas têm conteúdo que deixa menos que minWidgetWidth.
    let sizes = vec![sz(460.0, 20.0), sz(470.0, 20.0), sz(480.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);
    let result = context.try_find_widget_outline(2, 15.0, 500.0, DEFAULT_LAYOUT_CONSTANTS);

    assert_eq!(result, None);
}

#[test]
fn try_find_widget_outline_finds_outline_on_target_line_when_it_has_enough_space() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0), sz(100.0, 20.0), sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);
    let result = context.try_find_widget_outline(2, 15.0, 500.0, DEFAULT_LAYOUT_CONSTANTS);

    assert!(result.is_some());
    assert!(f64::from(result.unwrap().horizontal_widget_range.length()) >= DEFAULT_LAYOUT_CONSTANTS.min_widget_width);
}

#[test]
fn try_find_widget_outline_searches_outward_from_target_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // A primeira e a última linha ficam fora do posicionamento.
    // As linhas 2 e 3 não têm espaço, a linha 4 tem.
    let sizes = vec![
        sz(100.0, 20.0), // line 1 - excluded (first)
        sz(460.0, 20.0), // line 2 - no space
        sz(460.0, 20.0), // line 3 - no space (target)
        sz(100.0, 20.0), // line 4 - has space
        sz(100.0, 20.0), // line 5 - has space
        sz(100.0, 20.0), // line 6 - has space
        sz(100.0, 20.0), // line 7 - excluded (last)
    ];
    let line_range_info = create_line_range_info(1, sizes, 0.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);
    // O alvo é a linha 3, mas deve achar a linha 4 (busca para fora).
    let result = context.try_find_widget_outline(3, 15.0, 500.0, DEFAULT_LAYOUT_CONSTANTS);

    assert!(result.is_some());
}

#[test]
fn try_find_widget_outline_prefers_closer_lines_to_target() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![
        sz(100.0, 20.0),   // line 0 - excluded (first)
        sz(100.0, 20.0),   // line 1 - has space
        sz(100.0, 20.0),   // line 2 - has space
        sz(100.0, 20.0),   // line 3 - has space
        sz(500.0, 9999.0), // line 4 - no space (target)
        sz(100.0, 20.0),   // line 5 - has space
        sz(100.0, 20.0),   // line 6 - has space
        sz(100.0, 20.0),   // line 7 - has space
        sz(100.0, 20.0),   // line 8 - excluded (last)
    ];
    let line_range_info = create_line_range_info(1, sizes, 0.0);

    for target_line in 0..=4 {
        let context = WidgetPlacementContext::new(line_range_info.clone(), 500.0, |_| 0.0);
        let result = context.try_find_widget_outline(target_line, 15.0, 500.0, DEFAULT_LAYOUT_CONSTANTS);
        assert!(result.is_some());
        assert!(result.unwrap().vertical_widget_range.end_exclusive < 9999);
    }

    for target_line in 5..=10 /* test outside line range */ {
        let context = WidgetPlacementContext::new(line_range_info.clone(), 500.0, |_| 0.0);
        let result = context.try_find_widget_outline(target_line, 15.0, 500.0, DEFAULT_LAYOUT_CONSTANTS);
        assert!(result.is_some());
        assert!(result.unwrap().vertical_widget_range.start > 9999);
    }
}

#[test]
fn try_find_widget_outline_horizontal_widget_range_ends_at_editor_content_right() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0), sz(100.0, 20.0), sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);
    let editor_true_content_right = 500.0;

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);
    let result = context.try_find_widget_outline(2, 15.0, editor_true_content_right, DEFAULT_LAYOUT_CONSTANTS);

    assert!(result.is_some());
    assert_eq!(f64::from(result.unwrap().horizontal_widget_range.end_exclusive), editor_true_content_right);
}

// edge cases

#[test]
fn edge_cases_handles_single_line_range() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(5, sizes, 50.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);

    assert_eq!(context.available_space_sizes.len(), 1);
    assert_eq!(context.available_space_height_prefix_sums, vec![0.0, 20.0]);
}

#[test]
fn edge_cases_handles_empty_content_lines_width_0() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(0.0, 20.0), sz(0.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 0.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);

    assert_eq!(context.available_space_sizes[0].width, 500.0);
    assert_eq!(context.available_space_sizes[1].width, 500.0);
}

#[test]
fn edge_cases_handles_varying_line_heights() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 10.0), sz(100.0, 30.0), sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(1, sizes, 100.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);

    // Confere que as somas de prefixo consideram as alturas variadas.
    assert_eq!(context.available_space_height_prefix_sums, vec![0.0, 10.0, 40.0, 60.0]);
}

#[test]
fn edge_cases_handles_very_large_line_numbers() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes = vec![sz(100.0, 20.0)];
    let line_range_info = create_line_range_info(10000, sizes, 0.0);

    let context = WidgetPlacementContext::new(line_range_info, 500.0, |_| 0.0);

    let outline = context.get_widget_vertical_outline(10000, 50.0, DEFAULT_LAYOUT_CONSTANTS);
    assert!(outline.is_some());
}
