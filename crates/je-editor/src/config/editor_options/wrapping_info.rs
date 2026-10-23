//! Porte de `EditorWrappingInfo` e `EditorWrappingInfoComputer` (`editorOptions.ts`, linhas 5668 a 5700):
//! opção só calculada, que copia do ambiente e do `layoutInfo` o que o modelo precisa para quebrar linhas.

use crate::config::editor_options::layout_info::LAYOUT_INFO;
use crate::config::editor_options::{ComputedEditorOption, EditorOption, Opt};

/// Chave tipada de `wrappingInfo`.
pub const WRAPPING_INFO: Opt<EditorWrappingInfo> = Opt::new(EditorOption::WrappingInfo);

/// `EditorWrappingInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorWrappingInfo {
    pub is_dominated_by_long_lines: bool,
    pub is_word_wrap_minified: bool,
    pub is_viewport_wrapping: bool,
    pub wrapping_column: f64,
}

/// `EditorWrappingInfoComputer`.
pub fn editor_wrapping_info_computer() -> ComputedEditorOption<EditorWrappingInfo> {
    ComputedEditorOption::new(
        EditorOption::WrappingInfo,
        EditorWrappingInfo {
            is_dominated_by_long_lines: false,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,
        },
        |env, options, _| {
            let layout_info = options.get(LAYOUT_INFO);
            EditorWrappingInfo {
                is_dominated_by_long_lines: env.is_dominated_by_long_lines,
                is_word_wrap_minified: layout_info.is_word_wrap_minified,
                is_viewport_wrapping: layout_info.is_viewport_wrapping,
                wrapping_column: layout_info.wrapping_column,
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::editor_options::layout_info::{EditorLayoutInfo, EditorMinimapLayoutInfo, OverviewRulerPosition, RenderMinimap};
    use crate::config::editor_options::option_support::testing::{environment, font_info, options_with, value};
    use crate::config::editor_options::EditorOptionImpl;
    use je_base::dynamic::Dynamic;

    fn layout(is_word_wrap_minified: bool, is_viewport_wrapping: bool, wrapping_column: f64) -> EditorLayoutInfo {
        EditorLayoutInfo {
            width: 800.0,
            height: 600.0,
            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,
            line_numbers_left: 0.0,
            line_numbers_width: 40.0,
            decorations_left: 40.0,
            decorations_width: 10.0,
            content_left: 50.0,
            content_width: 700.0,
            minimap: EditorMinimapLayoutInfo {
                render_minimap: RenderMinimap::None,
                minimap_left: 0.0,
                minimap_width: 0.0,
                minimap_height_is_editor_height: false,
                minimap_is_sampling: false,
                minimap_scale: 1.0,
                minimap_line_height: 1.0,
                minimap_canvas_inner_width: 0.0,
                minimap_canvas_inner_height: 600.0,
                minimap_canvas_outer_width: 0.0,
                minimap_canvas_outer_height: 600.0,
            },
            viewport_column: 80.0,
            is_word_wrap_minified,
            is_viewport_wrapping,
            wrapping_column,
            vertical_scrollbar_width: 14.0,
            horizontal_scrollbar_height: 10.0,
            overview_ruler: OverviewRulerPosition { width: 14.0, height: 600.0, top: 0.0, right: 0.0 },
        }
    }

    #[test]
    fn identity_and_default() {
        let option = editor_wrapping_info_computer();
        assert_eq!(option.id(), EditorOption::WrappingInfo);
        assert_eq!(option.name(), "_never_");
        assert!(option.schema().is_none());
        assert_eq!(
            option.default_value(),
            EditorWrappingInfo {
                is_dominated_by_long_lines: false,
                is_word_wrap_minified: false,
                is_viewport_wrapping: false,
                wrapping_column: -1.0,
            }
        );
        assert_eq!(option.validate(&Dynamic::Bool(true)), option.default_value());
    }

    #[test]
    fn compute_combines_the_environment_and_the_layout() {
        let option = editor_wrapping_info_computer();
        let mut env = environment(font_info(true, 14.0, "normal"));
        let options = options_with(vec![(EditorOption::LayoutInfo, value(layout(true, false, 120.0)))]);
        env.is_dominated_by_long_lines = true;
        let computed = option.compute(&env, &options, &option.default_value());
        assert_eq!(
            computed,
            EditorWrappingInfo {
                is_dominated_by_long_lines: true,
                is_word_wrap_minified: true,
                is_viewport_wrapping: false,
                wrapping_column: 120.0,
            }
        );

        env.is_dominated_by_long_lines = false;
        let options = options_with(vec![(EditorOption::LayoutInfo, value(layout(false, true, 77.0)))]);
        let computed = option.compute(&env, &options, &computed);
        assert_eq!(
            computed,
            EditorWrappingInfo {
                is_dominated_by_long_lines: false,
                is_word_wrap_minified: false,
                is_viewport_wrapping: true,
                wrapping_column: 77.0,
            }
        );
    }
}
