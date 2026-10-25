//! Porte de `src/vs/editor/test/browser/view/whitespace.test.ts` (suíte `WhitespaceOverlay`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `IEditorOptions` é o objeto `Dynamic` das opções (`{ ...A, ...options }` é [`merge`], que sobrescreve as chaves
//!   repetidas) e `testViewModel` é `test_view_model` de `tests/support/test_view_model.rs`.
//! - O `render` do overlay devolve o HTML da linha no original, que os casos leem pelo DOM; a UI é nativa e o
//!   resultado de cada linha é uma `WhitespaceLine` (ver `whitespace.rs`): `element.querySelectorAll('svg')`,
//!   `'svg > *'`, `'path'`, o `viewBox` e o `d` do `path` são os campos do `WhitespaceSvg`, e `.mwh` são os
//!   `WhitespaceMarker`. A linha que o original deixa com `innerHTML` vazio é `WhitespaceLine::Empty`.
//! - `new ViewContext(configuration, new TestColorTheme(), viewModel)` usa o tema do porte (`je_theme::ColorTheme`,
//!   sem cores), que é o que o `ViewContext` guarda.
//! - O `try { ... } finally { overlay.dispose() }` é um guarda com `Drop`, que descarta também quando o corpo do caso
//!   entra em pânico.
//! - O `for (const [mode, expected] of [...]) test(...)` vira um `#[test]` por modo, sobre o mesmo corpo.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_editor::config::editor_options::editor_font_info::FONT_INFO;
use je_editor::config::editor_options::line_height::LINE_HEIGHT;
use je_editor::cursor_columns::visible_column_from_column;
use je_editor::cursor_common::{CursorSimpleModel, EditorConfiguration};
use je_editor::dynamic_view_overlay::DynamicViewOverlay;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::rendering_context::{HorizontalPosition, IViewLines, LineVisibleRanges, RenderingContext};
use je_editor::selection::Selection;
use je_editor::view_events::{ViewCursorStateChangedEvent, ViewEventHandler};
use je_editor::view_lines_viewport_data::ViewportData;
use je_editor::view_model_types::{CursorChangeReason, IPartialViewLinesViewportData, IViewModel};
use je_editor::whitespace::{WhitespaceLine, WhitespaceOverlay};
use je_theme::{ColorTheme, ThemeType};
use support::test_view_model::{TestViewModel, test_view_model};

type Overlay = WhitespaceOverlay<TestViewModel>;

/// `IViewLines` do teste: `linesVisibleRangesForRange` devolve `null` e `visibleRangeForPosition` converte a coluna
/// visível (com o tamanho do tab do modelo) em pixels, pela largura do espaço.
struct TestViewLines {
    view_model: Rc<TestViewModel>,
    tab_size: i32,
    space_width: f64,
}

impl IViewLines for TestViewLines {
    fn lines_visible_ranges_for_range(&self, _range: Range, _include_new_lines: bool) -> Option<Vec<LineVisibleRanges>> {
        None
    }

    fn visible_range_for_position(&self, position: Position) -> Option<HorizontalPosition> {
        let line_content = self.view_model.get_line_content(position.line_number);
        let visible_column = visible_column_from_column(line_content.units(), position.column, self.tab_size);
        Some(HorizontalPosition::new(false, f64::from(visible_column) * self.space_width))
    }
}

/// O `finally { overlay.dispose(); }` do `renderWhitespace`.
struct DisposeOnDrop(Rc<Overlay>);

impl Drop for DisposeOnDrop {
    fn drop(&mut self) {
        self.0.dispose();
    }
}

/// `{ ...base, key: value, ... }`.
fn merge(base: &Dynamic, extra: &[(&str, Dynamic)]) -> Dynamic {
    let mut entries: Vec<(JsString, Dynamic)> = base.own_entries();
    for (key, value) in extra {
        let key = JsString::from(*key);
        match entries.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = value.clone(),
            None => entries.push((key, value.clone())),
        }
    }
    Dynamic::object(entries)
}

/// `renderWhitespace(text, options, selections)`.
fn render_whitespace(text: &[&str], options: &[(&str, Dynamic)], selections: &[Selection]) -> Vec<WhitespaceLine> {
    let defaults = Dynamic::object([
        ("lineHeight", Dynamic::from(19)),
        ("renderWhitespace", Dynamic::from("all")),
        ("experimentalWhitespaceRendering", Dynamic::from("svg")),
    ]);
    let options = merge(&defaults, options);
    let mut result = Vec::new();
    test_view_model(text, &options, |view_model, model, configuration| {
        let editor_configuration: Rc<dyn EditorConfiguration> = configuration.clone();
        let theme = Rc::new(ColorTheme::unloaded("test", ThemeType::Dark, HashMap::new()));
        let view_context = Rc::new(je_editor::view_context::ViewContext::new(
            editor_configuration.clone(),
            theme,
            view_model.clone(),
        ));
        let overlay = WhitespaceOverlay::new(view_context);
        let _dispose = DisposeOnDrop(overlay.clone());

        let line_count = view_model.get_line_count();
        let space_width = editor_configuration.options().get(FONT_INFO).space_width;
        let line_height = *editor_configuration.options().get(LINE_HEIGHT);
        let view_lines = TestViewLines {
            view_model: view_model.clone(),
            tab_size: model.get_options().tab_size,
            space_width,
        };
        let viewport_data = ViewportData::new(
            selections.to_vec(),
            IPartialViewLinesViewportData {
                big_numbers_delta: 0.0,
                start_line_number: 1,
                end_line_number: line_count,
                relative_vertical_offset: (0..line_count).map(|i| f64::from(i) * line_height).collect(),
                centered_line_number: 1,
                completely_visible_start_line_number: 1,
                completely_visible_end_line_number: line_count,
                line_height,
            },
            Vec::new(),
            view_model.clone(),
        );

        overlay.on_cursor_state_changed(&ViewCursorStateChangedEvent::new(
            selections.to_vec(),
            selections.to_vec(),
            CursorChangeReason::NotSet,
        ));
        overlay.prepare_render(&RenderingContext::new(
            view_model.view_layout(),
            viewport_data,
            Rc::new(view_lines),
            None,
        ));
        for line_number in 1..=line_count {
            result.push(overlay.render(1, line_number));
        }
    });
    result
}

/// `glyphCount(element)`: quantos `M` (um por espaço ou tab) o `d` do `path` tem.
fn glyph_count(line: &WhitespaceLine) -> usize {
    match line {
        WhitespaceLine::Svg(svg) => svg.path.matches('M').count(),
        _ => 0,
    }
}

/// `element.querySelectorAll('svg > *').length`: o `path`, que só existe quando há o que desenhar.
fn shape_count(line: &WhitespaceLine) -> usize {
    match line {
        WhitespaceLine::Svg(svg) => usize::from(!svg.path.is_empty()),
        _ => 0,
    }
}

/// `element.querySelectorAll('svg').length`.
fn svg_count(line: &WhitespaceLine) -> usize {
    usize::from(matches!(line, WhitespaceLine::Svg(_)))
}

#[test]
fn batches_whitespace_into_one_svg_path_per_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = render_whitespace(&[&" ".repeat(1000)], &[], &[]);
    let element = &lines[0];
    // `paths` é `querySelectorAll('path').length`, igual a `shapes` porque o `path` é o único filho do `svg`.
    assert_eq!(
        (svg_count(element), shape_count(element), shape_count(element), glyph_count(element)),
        (1, 1, 1, 1000)
    );
}

#[test]
fn leaves_svgs_empty_on_lines_without_whitespace() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let inner_html: Vec<Option<String>> = render_whitespace(&["", "text"], &[], &[])
        .iter()
        .map(|line| match line {
            WhitespaceLine::Svg(svg) => Some(svg.path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(inner_html, vec![Some(String::new()), Some(String::new())]);
}

#[test]
fn preserves_circle_and_tab_arrow_geometry_in_separate_closed_subpaths() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = render_whitespace(&[" \t "], &[], &[]);
    let WhitespaceLine::Svg(svg) = &lines[0] else {
        panic!("esperava um svg, obtido {:?}", lines[0]);
    };
    assert_eq!(
        (svg.view_box(), svg.path.trim().to_owned()),
        (
            "0 0 50 19".to_owned(),
            concat!(
                "M 3.57 9.50 a 1.43 1.43 0 1 0 2.86 0 a 1.43 1.43 0 1 0 -2.86 0 Z ",
                "M 10.00 10.21 L 18.00 10.21 L 16.40 11.81 L 17.20 12.61 L 20.00 9.81 L 20.00 9.19 L 17.20 6.39 L 16.40 7.19 L 18.00 8.79 L 10.00 8.79 Z ",
                "M 43.57 9.50 a 1.43 1.43 0 1 0 2.86 0 a 1.43 1.43 0 1 0 -2.86 0 Z"
            )
            .to_owned()
        )
    );
}

const TEXT: [&str; 5] = ["a  b", "\ta\tb", "a   ", "   ", "a b"];

/// O corpo do `test(`respects ${mode} whitespace rendering`)`.
fn assert_respects_whitespace_rendering(mode: &str, expected: [usize; 5]) {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let actual: Vec<(usize, usize)> = render_whitespace(&TEXT, &[("renderWhitespace", Dynamic::from(mode))], &[])
        .iter()
        .map(|line| (glyph_count(line), shape_count(line)))
        .collect();
    assert_eq!(actual, expected.map(|glyphs| (glyphs, usize::from(glyphs > 0))).to_vec());
}

#[test]
fn respects_all_whitespace_rendering() {
    assert_respects_whitespace_rendering("all", [2, 2, 3, 3, 1]);
}

#[test]
fn respects_boundary_whitespace_rendering() {
    assert_respects_whitespace_rendering("boundary", [2, 2, 3, 3, 0]);
}

#[test]
fn respects_trailing_whitespace_rendering() {
    assert_respects_whitespace_rendering("trailing", [0, 0, 3, 3, 0]);
}

#[test]
fn respects_none_whitespace_rendering() {
    assert_respects_whitespace_rendering("none", [0, 0, 0, 0, 0]);
}

#[test]
fn respects_selection_whitespace_rendering() {
    assert_respects_whitespace_rendering("selection", [0, 0, 0, 0, 0]);
}

#[test]
fn renders_only_the_selected_spaces_and_tabs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let selections = [Selection::new(1, 2, 1, 3), Selection::new(2, 1, 2, 2)];
    let glyphs: Vec<usize> =
        render_whitespace(&TEXT, &[("renderWhitespace", Dynamic::from("selection"))], &selections)
            .iter()
            .map(glyph_count)
            .collect();
    assert_eq!(glyphs, vec![1, 1, 0, 0, 0]);
}

#[test]
fn respects_stop_rendering_line_after() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let glyphs: Vec<usize> = render_whitespace(&["    text"], &[("stopRenderingLineAfter", Dynamic::from(2))], &[])
        .iter()
        .map(glyph_count)
        .collect();
    assert_eq!(glyphs, vec![2]);
}

#[test]
fn leaves_font_rendering_unchanged() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let lines = render_whitespace(&[" \t "], &[("experimentalWhitespaceRendering", Dynamic::from("font"))], &[]);
    let markers = match &lines[0] {
        WhitespaceLine::Markers(markers) => markers.len(),
        _ => 0,
    };
    // `markers` é `querySelectorAll('.mwh').length`.
    assert_eq!((svg_count(&lines[0]), markers), (0, 3));
}

#[test]
fn does_not_render_an_overlay_when_the_legacy_renderer_is_enabled() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        render_whitespace(&[" \t "], &[("experimentalWhitespaceRendering", Dynamic::from("off"))], &[]),
        vec![WhitespaceLine::Empty]
    );
}
