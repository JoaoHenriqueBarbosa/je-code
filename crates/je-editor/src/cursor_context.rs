//! Porte de `src/vs/editor/common/cursor/cursorContext.ts`: o que os cursores precisam do ambiente (o modelo
//! de texto, o modelo de visão, o conversor de coordenadas e a configuração).
//!
//! Decisões de tradução:
//!
//! - Os objetos do original são compartilhados entre o controlador e os cursores, então os campos são
//!   `Rc`: `Rc<TextModel>` para o `ITextModel`, `Rc<dyn CursorSimpleModel>` para o `ICursorSimpleModel`,
//!   `Rc<dyn ICoordinatesConverter>` e `Rc<CursorConfiguration>` (a configuração não é `Clone`).
//! - O campo `_cursorContextBrand: void` (marca de tipagem nominal) não existe.

use std::rc::Rc;

use crate::coordinates_converter::ICoordinatesConverter;
use crate::cursor_common::{CursorConfiguration, CursorSimpleModel};
use crate::text_model::TextModel;

pub struct CursorContext {
    pub model: Rc<TextModel>,
    pub view_model: Rc<dyn CursorSimpleModel>,
    pub coordinates_converter: Rc<dyn ICoordinatesConverter>,
    pub cursor_config: Rc<CursorConfiguration>,
}

impl CursorContext {
    pub fn new(
        model: Rc<TextModel>,
        view_model: Rc<dyn CursorSimpleModel>,
        coordinates_converter: Rc<dyn ICoordinatesConverter>,
        cursor_config: Rc<CursorConfiguration>,
    ) -> Self {
        Self { model, view_model, coordinates_converter, cursor_config }
    }
}

/// Montagem compartilhada pelos testes de `one_cursor.rs` e `cursor_collection.rs`: um contexto sobre um
/// modelo de texto real, com o `IdentityCoordinatesConverter`.
#[cfg(test)]
pub(crate) mod test_support {
    use std::any::Any;
    use std::cell::RefCell;
    use std::collections::HashMap;

    use je_base::JsString;
    use je_base::event::{Emitter, Event};
    use je_base::lifecycle::{DisposableRef, mark_as_singleton, to_disposable};

    use super::*;
    use crate::config::editor_options::layout_info::{
        EditorLayoutInfo, EditorMinimapLayoutInfo, OverviewRulerPosition, RenderMinimap,
    };
    use crate::config::editor_options::option_support::testing;
    use crate::config::editor_options::{ComputedEditorOptions, EditorAutoIndentStrategy, EditorOption};
    use crate::coordinates_converter::IdentityCoordinatesConverter;
    use crate::cursor_common::EditorConfiguration;
    use crate::language_configuration::LanguageConfiguration;
    use crate::language_configuration_registry::{
        ILanguageConfigurationService, LanguageConfigurationServiceChangeEvent, ResolvedLanguageConfiguration,
    };
    use crate::model::{
        BracketPairColorizationOptions, DefaultEndOfLine, IndentSize, PositionAffinity, TextModelResolvedOptions,
    };
    use crate::position::Position;
    use crate::undo_redo_service::UndoRedoService;

    struct TestLanguageConfigurationService {
        on_did_change: Rc<Emitter<LanguageConfigurationServiceChangeEvent>>,
        configurations: RefCell<HashMap<String, Rc<ResolvedLanguageConfiguration>>>,
    }

    impl ILanguageConfigurationService for TestLanguageConfigurationService {
        fn on_did_change(&self) -> Event<LanguageConfigurationServiceChangeEvent> {
            self.on_did_change.event()
        }

        fn register(
            &self,
            language_id: &str,
            configuration: LanguageConfiguration,
            _priority: Option<i32>,
        ) -> DisposableRef {
            self.configurations.borrow_mut().insert(
                language_id.to_owned(),
                Rc::new(ResolvedLanguageConfiguration::new(language_id, configuration)),
            );
            mark_as_singleton(to_disposable(|| {}))
        }

        fn get_language_configuration(&self, language_id: &str) -> Rc<ResolvedLanguageConfiguration> {
            Rc::new(ResolvedLanguageConfiguration::new(language_id, LanguageConfiguration::default()))
        }
    }

    struct TestConfiguration {
        options: ComputedEditorOptions,
    }

    impl EditorConfiguration for TestConfiguration {
        fn options(&self) -> &ComputedEditorOptions {
            &self.options
        }
    }

    fn layout() -> EditorLayoutInfo {
        EditorLayoutInfo {
            width: 600.0,
            height: 200.0,
            glyph_margin_left: 0.0,
            glyph_margin_width: 0.0,
            glyph_margin_decoration_lane_count: 1.0,
            line_numbers_left: 0.0,
            line_numbers_width: 0.0,
            decorations_left: 0.0,
            decorations_width: 0.0,
            content_left: 0.0,
            content_width: 600.0,
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
            viewport_column: 80.0,
            is_word_wrap_minified: false,
            is_viewport_wrapping: false,
            wrapping_column: -1.0,
            vertical_scrollbar_width: 0.0,
            horizontal_scrollbar_height: 0.0,
            overview_ruler: OverviewRulerPosition { width: 0.0, height: 0.0, top: 0.0, right: 0.0 },
        }
    }

    fn text(value: &str) -> Rc<dyn Any> {
        testing::value(value.to_owned())
    }

    fn options(merge_overlapping: bool) -> ComputedEditorOptions {
        testing::options_with(vec![
            (EditorOption::LayoutInfo, testing::value(layout())),
            (EditorOption::FontInfo, testing::value(testing::font_info(true, 14.0, "normal"))),
            (EditorOption::ReadOnly, testing::value(false)),
            (EditorOption::StickyTabStops, testing::value(true)),
            (EditorOption::UseTabStops, testing::value(true)),
            (EditorOption::TrimWhitespaceOnDelete, testing::value(false)),
            (EditorOption::WordSeparators, text("`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?")),
            (EditorOption::EmptySelectionClipboard, testing::value(true)),
            (EditorOption::CopyWithSyntaxHighlighting, testing::value(true)),
            (EditorOption::MultiCursorMergeOverlapping, testing::value(merge_overlapping)),
            (EditorOption::MultiCursorPaste, text("spread")),
            (EditorOption::MultiCursorLimit, testing::value(10000.0_f64)),
            (EditorOption::AutoClosingBrackets, text("always")),
            (EditorOption::AutoClosingComments, text("always")),
            (EditorOption::AutoClosingQuotes, text("always")),
            (EditorOption::AutoClosingDelete, text("auto")),
            (EditorOption::AutoClosingOvertype, text("auto")),
            (EditorOption::AutoSurround, text("languageDefined")),
            (EditorOption::AutoIndent, testing::value(EditorAutoIndentStrategy::Advanced)),
            (EditorOption::WordSegmenterLocales, testing::value(Vec::<String>::new())),
            (EditorOption::OvertypeOnPaste, testing::value(false)),
        ])
    }

    /// Modelo simples de teste: as linhas do texto, sem quebra de linha de visão.
    struct LinesModel {
        lines: Vec<String>,
    }

    impl LinesModel {
        fn units(&self, line_number: i32) -> Vec<u16> {
            self.lines[line_number as usize - 1].encode_utf16().collect()
        }
    }

    impl CursorSimpleModel for LinesModel {
        fn get_line_count(&self) -> i32 {
            self.lines.len() as i32
        }

        fn get_line_content(&self, line_number: i32) -> JsString {
            JsString::from_units(self.units(line_number))
        }

        fn get_line_min_column(&self, _line_number: i32) -> i32 {
            1
        }

        fn get_line_max_column(&self, line_number: i32) -> i32 {
            self.units(line_number).len() as i32 + 1
        }

        fn get_line_first_non_whitespace_column(&self, line_number: i32) -> i32 {
            self.units(line_number)
                .iter()
                .position(|&u| u != u16::from(b' ') && u != u16::from(b'\t'))
                .map_or(0, |index| index as i32 + 1)
        }

        fn get_line_last_non_whitespace_column(&self, line_number: i32) -> i32 {
            self.units(line_number)
                .iter()
                .rposition(|&u| u != u16::from(b' ') && u != u16::from(b'\t'))
                .map_or(0, |index| index as i32 + 2)
        }

        fn normalize_position(&self, position: Position, _affinity: PositionAffinity) -> Position {
            position
        }

        fn get_line_indent_column(&self, line_number: i32) -> i32 {
            match self.get_line_first_non_whitespace_column(line_number) {
                0 => self.get_line_max_column(line_number),
                column => column,
            }
        }
    }

    /// Um contexto sobre um modelo de texto real com o `text` dado.
    pub(crate) fn context_for(text: &str, merge_overlapping: bool) -> CursorContext {
        let model = TextModel::new(
            text.into(),
            "plaintext",
            TextModel::default_creation_options(),
            None,
            UndoRedoService::for_tests(),
        );
        let service: Rc<dyn ILanguageConfigurationService> = Rc::new(TestLanguageConfigurationService {
            on_did_change: Emitter::new(),
            configurations: RefCell::new(HashMap::new()),
        });
        let model_options = TextModelResolvedOptions::new(
            4,
            IndentSize::TabSize,
            true,
            DefaultEndOfLine::Lf,
            true,
            BracketPairColorizationOptions { enabled: false, independent_color_pool_per_bracket_type: false },
        );
        let cursor_config = CursorConfiguration::new(
            "plaintext",
            &model_options,
            &TestConfiguration { options: options(merge_overlapping) },
            service,
        );
        let view_model = LinesModel { lines: text.split('\n').map(str::to_owned).collect() };
        CursorContext::new(
            model.clone(),
            Rc::new(view_model),
            Rc::new(IdentityCoordinatesConverter::new(model)),
            Rc::new(cursor_config),
        )
    }
}
