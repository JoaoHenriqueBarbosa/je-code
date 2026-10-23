//! Porte de `src/vs/editor/common/viewModel.ts`: `IViewModel`, `IViewLayout`, `IEditorWhitespace`,
//! `IWhitespaceChangeAccessor`, `ILineHeightChangeAccessor`, `IPartialViewLinesViewportData`,
//! `IViewWhitespaceViewportData`, `Viewport`, `MinimapLinesRenderingData`, `ViewLineData`,
//! `ViewLineRenderingData`, `ViewModelDecoration` e `OverviewRulerDecorationsGroup`.
//!
//! Também traz `isModelDecorationVisible` (com `isModelDecorationInComment` e `isModelDecorationInString`) de
//! `src/vs/editor/common/viewModel/viewModelDecoration.ts`, que o `inlineDecorations.ts` consome, e os
//! enums pequenos que o `viewModel.ts` importa e ainda não tinham porte: `ScrollType` (`editorCommon.ts`),
//! `CursorChangeReason` (`cursorEvents.ts`) e `VerticalRevealType` (`viewEvents.ts`).
//!
//! Representação:
//! - Os `number` que o original reduz com `| 0` (números de linha, ordinal, altura e largura mínima de
//!   whitespace) são `i32` já no tipo. Posições verticais, somas de altura e a altura de linha (que pode ser
//!   fracionária) são `f64`. `Viewport` guarda `f64` já reduzidos por `| 0`, como o `Scrollable`.
//! - `IViewModel` e `IViewLayout` são traits com `&self`: o original muta estado por trás de uma interface
//!   compartilhada, então quem implementa usa mutabilidade interior (`RefCell`), como o resto do crate.
//! - `IViewModel extends ICursorSimpleModel, ISimpleModel`: os membros de `ICursorSimpleModel`
//!   (`getLineCount`, `getLineContent`, `getLineLength`, `getLineMinColumn`, `getLineMaxColumn`,
//!   `getLineFirstNonWhitespaceColumn`, `getLineLastNonWhitespaceColumn`) vêm do supertrait
//!   [`CursorSimpleModel`] e não são redeclarados; os de `ISimpleModel` que não estão nele
//!   (`getValueInRange`, `getValueLengthInRange`, `modifyPosition`) são declarados aqui.
//! - `ITextModel` é o `Rc<TextModel>`. Os tipos de `ViewEventHandler`, `EditorTheme` e
//!   `IGlyphMarginLanesModel`, que ainda não têm porte, entram como tipos associados do trait.
//! - `getEditorOption<T>(id)` (resultado tipado por opção) vira [`IViewModel::computed_editor_options`]: o
//!   chamador lê a opção pela constante tipada do registro, como em `cursor_common`.
//! - `string | null | undefined` (`source`) é `Option<&str>`; `minimalReveal?` é `Option<bool>`.
//! - Os campos `_xxxBrand: void` do original (marcas de tipagem nominal) não existem.

use std::rc::Rc;

use je_base::JsString;
use je_base::scrollable::{INewScrollPosition, IScrollPosition, Scrollable};
use je_base::strings;

use crate::config::editor_options::ComputedEditorOptions;
use crate::editor_common::ScrollType;
use crate::coordinates_converter::ICoordinatesConverter;
use crate::cursor_common::{
    ColumnSelectData, CursorConfiguration, CursorSimpleModel, CursorState, EditOperationType, PartialCursorState,
};
use crate::encoded_token_attributes::standard_token_type;
use crate::inline_decorations::InlineDecoration;
use crate::line_tokens::IViewLineTokens;
use crate::model::{EndOfLinePreference, ModelDecoration, TextDirection};
use crate::model_line_projection_data::{InjectedText, LineBreaksComputer, LineBreaksComputerContext};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::{ModelDecorationOptions, TextModel};
use crate::text_model_events::{InternalModelContentChangeEvent, ModelInjectedTextChangedEvent};
use crate::text_model_guides::{ActiveIndentGuideInfo, BracketGuideOptions, IndentGuide};


/// `CursorChangeReason` (`cursorEvents.ts`): por que o cursor mudou de posição.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorChangeReason {
    /// Desconhecido ou não definido.
    NotSet = 0,
    /// Um `model.setValue()` foi chamado.
    ContentFlush = 1,
    /// O modelo mudou fora deste cursor e o cursor recupera a posição dos marcadores associados.
    RecoverFromMarkers = 2,
    /// Houve um gesto explícito do usuário.
    Explicit = 3,
    /// Houve um Paste.
    Paste = 4,
    /// Houve um Undo.
    Undo = 5,
    /// Houve um Redo.
    Redo = 6,
}

/// `VerticalRevealType` (`viewEvents.ts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalRevealType {
    Simple = 0,
    Center = 1,
    CenterIfOutsideViewport = 2,
    Top = 3,
    Bottom = 4,
    NearTop = 5,
    NearTopIfOutsideViewport = 6,
}

/// `IEditorWhitespace`. No original, `getWhitespaces()` devolve os próprios objetos internos do layout; aqui
/// é uma cópia do estado no momento da chamada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IEditorWhitespace {
    pub id: String,
    pub after_line_number: i32,
    pub height: i32,
}

/// `IWhitespaceChangeAccessor`: insere, muda e remove whitespaces em lote.
pub trait IWhitespaceChangeAccessor {
    fn insert_whitespace(&mut self, after_line_number: i32, ordinal: i32, height_in_px: i32, min_width: i32) -> String;
    fn change_one_whitespace(&mut self, id: &str, new_after_line_number: i32, new_height: i32);
    fn remove_whitespace(&mut self, id: &str);
}

/// `ILineHeightChangeAccessor`: insere, muda e remove alturas de linha customizadas.
pub trait ILineHeightChangeAccessor {
    fn insert_or_change_custom_line_height(
        &mut self,
        decoration_id: &str,
        start_line_number: i32,
        end_line_number: i32,
        line_height: f64,
    );
    fn remove_custom_line_height(&mut self, decoration_id: &str);
}

/// `IPartialViewLinesViewportData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IPartialViewLinesViewportData {
    /// Valor a subtrair de `scrollTop` (para os números de offset vertical ficarem abaixo de 1MM).
    pub big_numbers_delta: f64,
    /// A primeira linha (parcialmente) visível.
    pub start_line_number: i32,
    /// A última linha (parcialmente) visível.
    pub end_line_number: i32,
    /// `relative_vertical_offset[i]` é o `top` da linha `i + start_line_number`.
    pub relative_vertical_offset: Vec<f64>,
    /// A linha centralizada no viewport.
    pub centered_line_number: i32,
    /// A primeira linha completamente visível.
    pub completely_visible_start_line_number: i32,
    /// A última linha completamente visível.
    pub completely_visible_end_line_number: i32,
    /// A altura de uma linha.
    pub line_height: f64,
}

/// `IViewWhitespaceViewportData`.
#[derive(Clone, Debug, PartialEq)]
pub struct IViewWhitespaceViewportData {
    pub id: String,
    pub after_line_number: i32,
    pub vertical_offset: f64,
    pub height: i32,
}

/// `ViewModelDecoration` (`viewModelDecoration.ts`): o intervalo da decoração na visão, com as opções do
/// modelo.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewModelDecoration {
    pub range: Range,
    pub options: Rc<ModelDecorationOptions>,
}

impl ViewModelDecoration {
    pub fn new(range: Range, options: Rc<ModelDecorationOptions>) -> Self {
        Self { range, options }
    }
}

/// `isModelDecorationVisible(model, decoration)`.
pub fn is_model_decoration_visible(model: &TextModel, decoration: &ModelDecoration) -> bool {
    if decoration.options.hide_in_comment_tokens.unwrap_or(false) && is_model_decoration_in_comment(model, decoration) {
        return false;
    }

    if decoration.options.hide_in_string_tokens.unwrap_or(false) && is_model_decoration_in_string(model, decoration) {
        return false;
    }

    true
}

/// `isModelDecorationInComment(model, decoration)`.
pub fn is_model_decoration_in_comment(model: &TextModel, decoration: &ModelDecoration) -> bool {
    test_tokens_in_range(model, &decoration.range, |token_type| token_type == standard_token_type::COMMENT)
}

/// `isModelDecorationInString(model, decoration)`.
pub fn is_model_decoration_in_string(model: &TextModel, decoration: &ModelDecoration) -> bool {
    test_tokens_in_range(model, &decoration.range, |token_type| token_type == standard_token_type::STRING)
}

/// `testTokensInRange(model, range, callback)`: chama o `callback` para cada token que cruza o intervalo.
/// Se o `callback` devolve `false`, a iteração para e o resultado é `false`; senão é `true`.
fn test_tokens_in_range(model: &TextModel, range: &Range, mut callback: impl FnMut(u32) -> bool) -> bool {
    for line_number in range.start_line_number..=range.end_line_number {
        let line_tokens = model.tokenization().get_line_tokens(line_number);
        let is_first_line = line_number == range.start_line_number;
        let is_end_line = line_number == range.end_line_number;

        let mut token_idx = if is_first_line { line_tokens.find_token_index_at_offset(range.start_column - 1) } else { 0 };
        while token_idx < line_tokens.get_count() {
            if is_end_line {
                let start_offset = line_tokens.get_start_offset(token_idx);
                if start_offset > range.end_column - 1 {
                    break;
                }
            }

            if !callback(line_tokens.get_standard_token_type(token_idx)) {
                return false;
            }
            token_idx += 1;
        }
    }
    true
}

/// `x | 0` do JavaScript sobre um `number`: `ToInt32` (trunca, dá a volta em 2^32; `NaN` e infinito viram 0).
fn to_int32(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    f64::from(value.trunc().rem_euclid(4_294_967_296.0) as u32 as i32)
}

/// `Viewport`: `top`, `left`, `width` e `height` já passam por `| 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
}

impl Viewport {
    pub fn new(top: f64, left: f64, width: f64, height: f64) -> Self {
        Self { top: to_int32(top), left: to_int32(left), width: to_int32(width), height: to_int32(height) }
    }
}

/// `MinimapLinesRenderingData`.
#[derive(Clone)]
pub struct MinimapLinesRenderingData {
    pub tab_size: i32,
    pub data: Vec<Option<ViewLineData>>,
}

impl MinimapLinesRenderingData {
    pub fn new(tab_size: i32, data: Vec<Option<ViewLineData>>) -> Self {
        Self { tab_size, data }
    }
}

/// `ViewLineData`.
#[derive(Clone)]
pub struct ViewLineData {
    /// O conteúdo desta linha da visão.
    pub content: JsString,
    /// Esta linha continua com uma linha quebrada?
    pub continues_with_wrapped_line: bool,
    /// A menor coluna permitida nesta linha da visão.
    pub min_column: i32,
    /// A maior coluna permitida nesta linha da visão.
    pub max_column: i32,
    /// A coluna visível no começo da linha (depois do fauxIndent).
    pub start_visible_column: i32,
    /// Os tokens desta linha da visão.
    pub tokens: Rc<dyn IViewLineTokens>,
    /// Decorações em linha adicionais desta linha.
    pub inline_decorations: Option<Vec<InlineDecoration>>,
}

impl ViewLineData {
    pub fn new(
        content: JsString,
        continues_with_wrapped_line: bool,
        min_column: i32,
        max_column: i32,
        start_visible_column: i32,
        tokens: Rc<dyn IViewLineTokens>,
        inline_decorations: Option<Vec<InlineDecoration>>,
    ) -> Self {
        Self { content, continues_with_wrapped_line, min_column, max_column, start_visible_column, tokens, inline_decorations }
    }
}

/// `ViewLineRenderingData`.
#[derive(Clone)]
pub struct ViewLineRenderingData {
    /// A menor coluna permitida nesta linha da visão.
    pub min_column: i32,
    /// A maior coluna permitida nesta linha da visão.
    pub max_column: i32,
    /// O conteúdo desta linha da visão.
    pub content: JsString,
    /// Esta linha continua com uma linha quebrada?
    pub continues_with_wrapped_line: bool,
    /// `content` contém caracteres RTL?
    pub contains_rtl: bool,
    /// `content` é só ASCII básico?
    pub is_basic_ascii: bool,
    /// Os tokens desta linha da visão.
    pub tokens: Rc<dyn IViewLineTokens>,
    /// As decorações em linha desta linha da visão.
    pub inline_decorations: Vec<InlineDecoration>,
    /// O tamanho do tab deste view model.
    pub tab_size: i32,
    /// A coluna visível no começo da linha (depois do fauxIndent).
    pub start_visible_column: i32,
    /// A direção a usar para renderizar a linha.
    pub text_direction: TextDirection,
    /// A linha tem fontes variáveis?
    pub has_variable_fonts: bool,
}

impl ViewLineRenderingData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        min_column: i32,
        max_column: i32,
        content: JsString,
        continues_with_wrapped_line: bool,
        might_contain_rtl: bool,
        might_contain_non_basic_ascii: bool,
        tokens: Rc<dyn IViewLineTokens>,
        inline_decorations: Vec<InlineDecoration>,
        tab_size: i32,
        start_visible_column: i32,
        text_direction: TextDirection,
        has_variable_fonts: bool,
    ) -> Self {
        let is_basic_ascii = Self::is_basic_ascii(content.units(), might_contain_non_basic_ascii);
        let contains_rtl = Self::contains_rtl(content.units(), is_basic_ascii, might_contain_rtl);
        Self {
            min_column,
            max_column,
            content,
            continues_with_wrapped_line,
            contains_rtl,
            is_basic_ascii,
            tokens,
            inline_decorations,
            tab_size,
            start_visible_column,
            text_direction,
            has_variable_fonts,
        }
    }

    pub fn is_basic_ascii(line_content: &[u16], might_contain_non_basic_ascii: bool) -> bool {
        if might_contain_non_basic_ascii {
            return strings::is_basic_ascii(line_content);
        }
        true
    }

    pub fn contains_rtl(line_content: &[u16], is_basic_ascii: bool, might_contain_rtl: bool) -> bool {
        if !is_basic_ascii && might_contain_rtl {
            return strings::contains_rtl(line_content);
        }
        false
    }
}

/// `OverviewRulerDecorationsGroup`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewRulerDecorationsGroup {
    pub color: String,
    pub z_index: i32,
    /// As decorações são codificadas num vetor de números neste esquema:
    ///  - `3*i` = lane
    ///  - `3*i+1` = startLineNumber
    ///  - `3*i+2` = endLineNumber
    pub data: Vec<i32>,
}

impl OverviewRulerDecorationsGroup {
    pub fn new(color: String, z_index: i32, data: Vec<i32>) -> Self {
        Self { color, z_index, data }
    }

    /// `compareByRenderingProps`: a cor compara por unidades UTF-16, como o `<` de strings do JavaScript.
    pub fn compare_by_rendering_props(a: &Self, b: &Self) -> i32 {
        if a.z_index == b.z_index {
            return match a.color.encode_utf16().cmp(b.color.encode_utf16()) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Equal => 0,
            };
        }
        a.z_index - b.z_index
    }

    pub fn equals(a: &Self, b: &Self) -> bool {
        a.color == b.color && a.z_index == b.z_index && a.data == b.data
    }

    pub fn equals_arr(a: &[Self], b: &[Self]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Self::equals(x, y))
    }
}

/// `IViewLayout`.
pub trait IViewLayout {
    fn get_scrollable(&self) -> Rc<Scrollable>;

    fn get_scroll_width(&self) -> f64;
    fn get_scroll_height(&self) -> f64;

    fn get_current_scroll_left(&self) -> f64;
    fn get_current_scroll_top(&self) -> f64;
    fn get_current_viewport(&self) -> Viewport;

    fn get_future_viewport(&self) -> Viewport;

    fn set_scroll_position(&self, position: INewScrollPosition, r#type: ScrollType);
    fn delta_scroll_now(&self, delta_scroll_left: f64, delta_scroll_top: f64);

    fn validate_scroll_position(&self, scroll_position: INewScrollPosition) -> IScrollPosition;

    fn set_max_line_width(&self, max_line_width: f64);
    fn set_overlay_widgets_min_width(&self, overlay_widgets_min_width: f64);

    fn get_lines_viewport_data(&self) -> IPartialViewLinesViewportData;
    fn get_lines_viewport_data_at_scroll_top(&self, scroll_top: f64) -> IPartialViewLinesViewportData;
    fn get_whitespaces(&self) -> Vec<IEditorWhitespace>;

    fn is_after_lines(&self, vertical_offset: f64) -> bool;
    fn is_in_top_padding(&self, vertical_offset: f64) -> bool;
    fn is_in_bottom_padding(&self, vertical_offset: f64) -> bool;
    fn get_line_number_at_vertical_offset(&self, vertical_offset: f64) -> i32;
    /// `includeViewZones?`: `None` é o `undefined` do original.
    fn get_vertical_offset_for_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64;
    /// `includeViewZones?`: `None` é o `undefined` do original.
    fn get_vertical_offset_after_line_number(&self, line_number: i32, include_view_zones: Option<bool>) -> f64;
    fn get_line_height_for_line_number(&self, line_number: i32) -> f64;
    fn get_whitespace_at_vertical_offset(&self, vertical_offset: f64) -> Option<IViewWhitespaceViewportData>;

    /// O layout dos whitespaces que estão no viewport agora.
    fn get_whitespace_viewport_data(&self) -> Vec<IViewWhitespaceViewportData>;
}

/// O argumento `InternalModelContentChangeEvent | ModelInjectedTextChangedEvent` de
/// `onDidChangeContentOrInjectedText` e `emitContentChangeEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentOrInjectedTextChangeEvent {
    Content(InternalModelContentChangeEvent),
    InjectedText(ModelInjectedTextChangedEvent),
}

/// `sourceText: string | string[]` do resultado de `getPlainTextToCopy`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlainTextSource {
    Text(JsString),
    Lines(Vec<JsString>),
}

/// O resultado de `getPlainTextToCopy`: `{ sourceRanges, sourceText }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlainTextToCopy {
    pub source_ranges: Vec<Range>,
    pub source_text: PlainTextSource,
}

/// O resultado de `getRichTextToCopy`: `{ html, mode }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichTextToCopy {
    pub html: String,
    pub mode: String,
}

/// `IViewModel`. Veja a nota de representação no topo do módulo sobre `&self` e os tipos associados.
pub trait IViewModel: CursorSimpleModel {
    /// O `ViewEventHandler` (`viewEventHandler.ts`), ainda sem porte.
    type EventHandler;
    /// O `EditorTheme` (`editorTheme.ts`), ainda sem porte.
    type EditorTheme;
    /// O `IGlyphMarginLanesModel` (`model.ts`), ainda sem porte.
    type GlyphLanes;

    fn model(&self) -> Rc<TextModel>;

    fn coordinates_converter(&self) -> Rc<dyn ICoordinatesConverter>;

    fn view_layout(&self) -> Rc<dyn IViewLayout>;

    fn cursor_config(&self) -> CursorConfiguration;

    fn glyph_lanes(&self) -> Rc<Self::GlyphLanes>;

    fn add_view_event_handler(&self, event_handler: Rc<Self::EventHandler>);
    fn remove_view_event_handler(&self, event_handler: &Rc<Self::EventHandler>);

    /// A tradução de `getEditorOption(id)`: as opções calculadas, lidas pelas constantes do registro.
    fn computed_editor_options(&self) -> ComputedEditorOptions;

    /// Dá a dica de que muitos pedidos vão chegar para estes números de linha.
    fn set_viewport(&self, start_line_number: i32, end_line_number: i32, centered_line_number: i32);
    fn visible_lines_stabilized(&self);
    fn set_has_focus(&self, has_focus: bool);
    fn set_has_widget_focus(&self, has_widget_focus: bool);
    fn on_composition_start(&self);
    fn on_composition_end(&self);

    fn get_font_size_at_position(&self, position: Position) -> Option<String>;
    fn get_minimap_decorations_in_range(&self, range: Range) -> Vec<ViewModelDecoration>;
    fn get_decorations_in_viewport(&self, visible_range: Range) -> Vec<ViewModelDecoration>;
    fn get_text_direction(&self, line_number: i32) -> TextDirection;
    fn get_viewport_view_line_rendering_data(&self, visible_range: Range, line_number: i32) -> ViewLineRenderingData;
    fn get_view_line_rendering_data(&self, line_number: i32) -> ViewLineRenderingData;
    fn get_view_line_continues_with_wrapped_line(&self, line_number: i32) -> bool;
    fn get_view_line_data(&self, line_number: i32) -> ViewLineData;
    fn get_minimap_lines_rendering_data(&self, start_line_number: i32, end_line_number: i32, needed: &[bool]) -> MinimapLinesRenderingData;
    fn get_completely_visible_view_range(&self) -> Range;
    fn get_completely_visible_view_range_at_scroll_top(&self, scroll_top: f64) -> Range;
    fn get_view_range_with_cursor_padding(&self, view_range: Range) -> Range;

    fn get_hidden_areas(&self) -> Vec<Range>;

    fn get_active_indent_guide(&self, line_number: i32, min_line_number: i32, max_line_number: i32) -> ActiveIndentGuideInfo;
    fn get_lines_indent_guides(&self, start_line_number: i32, end_line_number: i32) -> Vec<i32>;
    fn get_bracket_guides_in_range_by_line(
        &self,
        start_line_number: i32,
        end_line_number: i32,
        active_position: Option<Position>,
        options: BracketGuideOptions,
    ) -> Vec<Vec<IndentGuide>>;
    fn get_all_overview_ruler_decorations(&self, theme: &Self::EditorTheme) -> Vec<OverviewRulerDecorationsGroup>;
    /// De `ISimpleModel`.
    fn get_value_in_range(&self, range: Range, eol: EndOfLinePreference) -> JsString;
    /// De `ISimpleModel`.
    fn get_value_length_in_range(&self, range: Range, eol: EndOfLinePreference) -> usize;
    /// De `ISimpleModel`.
    fn modify_position(&self, position: Position, offset: i32) -> Position;

    fn get_injected_text_at(&self, view_position: Position) -> Option<InjectedText>;

    fn deduce_model_position_relative_to_view_position(&self, view_anchor_position: Position, delta_offset: i32, line_feed_cnt: i32) -> Position;
    fn get_plain_text_to_copy(&self, model_ranges: &[Range], empty_selection_clipboard: bool, force_crlf: bool) -> PlainTextToCopy;
    fn get_rich_text_to_copy(&self, model_ranges: &[Range], empty_selection_clipboard: bool) -> Option<RichTextToCopy>;

    fn on_did_change_content_or_injected_text(&self, e: ContentOrInjectedTextChangeEvent);
    fn emit_content_change_event(&self, e: ContentOrInjectedTextChangeEvent);

    fn create_line_breaks_computer(&self, context: Option<Rc<dyn LineBreaksComputerContext>>) -> Box<dyn LineBreaksComputer>;

    // #region cursor
    fn get_primary_cursor_state(&self) -> CursorState;
    fn get_last_added_cursor_index(&self) -> usize;
    fn get_cursor_states(&self) -> Vec<CursorState>;
    fn set_cursor_states(&self, source: Option<&str>, reason: CursorChangeReason, states: Option<Vec<PartialCursorState>>) -> bool;
    fn get_cursor_column_select_data(&self) -> ColumnSelectData;
    fn get_cursor_auto_closed_characters(&self) -> Vec<Range>;
    fn set_cursor_column_select_data(&self, column_select_data: ColumnSelectData);
    fn get_prev_edit_operation_type(&self) -> EditOperationType;
    fn set_prev_edit_operation_type(&self, r#type: EditOperationType);
    fn reveal_all_cursors(&self, source: Option<&str>, reveal_horizontal: bool, minimal_reveal: Option<bool>);
    fn reveal_primary_cursor(&self, source: Option<&str>, reveal_horizontal: bool, minimal_reveal: Option<bool>);
    fn reveal_top_most_cursor(&self, source: Option<&str>);
    fn reveal_bottom_most_cursor(&self, source: Option<&str>);
    fn reveal_range(&self, source: Option<&str>, reveal_horizontal: bool, view_range: Range, vertical_type: VerticalRevealType, scroll_type: ScrollType);
    // #endregion

    // #region viewLayout
    fn change_whitespace(&self, callback: &mut dyn FnMut(&mut dyn IWhitespaceChangeAccessor));
    // #endregion

    fn batch_events(&self, callback: &mut dyn FnMut());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn group(color: &str, z_index: i32, data: Vec<i32>) -> OverviewRulerDecorationsGroup {
        OverviewRulerDecorationsGroup::new(color.to_string(), z_index, data)
    }

    #[test]
    fn compare_by_rendering_props_uses_z_index_first() {
        // a.zIndex !== b.zIndex: devolve a.zIndex - b.zIndex, sem olhar a cor.
        assert_eq!(OverviewRulerDecorationsGroup::compare_by_rendering_props(&group("#b", 1, vec![]), &group("#a", 3, vec![])), -2);
        assert_eq!(OverviewRulerDecorationsGroup::compare_by_rendering_props(&group("#a", 5, vec![]), &group("#b", 2, vec![])), 3);
    }

    #[test]
    fn compare_by_rendering_props_breaks_ties_by_color() {
        assert_eq!(OverviewRulerDecorationsGroup::compare_by_rendering_props(&group("#a", 1, vec![]), &group("#b", 1, vec![])), -1);
        assert_eq!(OverviewRulerDecorationsGroup::compare_by_rendering_props(&group("#b", 1, vec![]), &group("#a", 1, vec![])), 1);
        assert_eq!(OverviewRulerDecorationsGroup::compare_by_rendering_props(&group("#a", 1, vec![1]), &group("#a", 1, vec![2])), 0);
    }

    #[test]
    fn compare_by_rendering_props_orders_colors_by_utf16_units() {
        // Em UTF-16, U+1F600 começa pelo surrogate 0xD83D, menor que 0xFF5E, então vem antes (em UTF-8 viria depois).
        let astral = "\u{1F600}";
        let bmp = "\u{FF5E}";
        assert_eq!(OverviewRulerDecorationsGroup::compare_by_rendering_props(&group(astral, 0, vec![]), &group(bmp, 0, vec![])), -1);
    }

    #[test]
    fn equals_compares_color_z_index_and_data() {
        let a = group("#a", 1, vec![1, 2, 3]);
        assert!(OverviewRulerDecorationsGroup::equals(&a, &group("#a", 1, vec![1, 2, 3])));
        assert!(!OverviewRulerDecorationsGroup::equals(&a, &group("#b", 1, vec![1, 2, 3])));
        assert!(!OverviewRulerDecorationsGroup::equals(&a, &group("#a", 2, vec![1, 2, 3])));
        assert!(!OverviewRulerDecorationsGroup::equals(&a, &group("#a", 1, vec![1, 2])));
        assert!(!OverviewRulerDecorationsGroup::equals(&a, &group("#a", 1, vec![1, 2, 4])));
    }

    #[test]
    fn equals_arr_requires_same_length_and_pairwise_equality() {
        let a = vec![group("#a", 1, vec![1]), group("#b", 2, vec![2])];
        assert!(OverviewRulerDecorationsGroup::equals_arr(&a, &a.clone()));
        assert!(OverviewRulerDecorationsGroup::equals_arr(&[], &[]));
        assert!(!OverviewRulerDecorationsGroup::equals_arr(&a, &a[..1]));
        let b = vec![group("#a", 1, vec![1]), group("#b", 2, vec![3])];
        assert!(!OverviewRulerDecorationsGroup::equals_arr(&a, &b));
    }

    #[test]
    fn is_basic_ascii_only_checks_when_content_might_be_non_basic() {
        assert!(ViewLineRenderingData::is_basic_ascii(&units("abc"), true));
        assert!(!ViewLineRenderingData::is_basic_ascii(&units("é"), true));
        // mightContainNonBasicASCII = false: assume ASCII sem olhar o texto.
        assert!(ViewLineRenderingData::is_basic_ascii(&units("é"), false));
    }

    #[test]
    fn contains_rtl_needs_non_basic_ascii_and_might_contain_rtl() {
        let hebrew = units("\u{05D0}");
        assert!(ViewLineRenderingData::contains_rtl(&hebrew, false, true));
        assert!(!ViewLineRenderingData::contains_rtl(&hebrew, true, true));
        assert!(!ViewLineRenderingData::contains_rtl(&hebrew, false, false));
        assert!(!ViewLineRenderingData::contains_rtl(&units("abc"), false, true));
    }

    #[test]
    fn viewport_truncates_with_or_zero() {
        let viewport = Viewport::new(10.9, -3.7, 100.2, 4_294_967_297.0);
        assert_eq!(viewport, Viewport { top: 10.0, left: -3.0, width: 100.0, height: 1.0 });
        assert_eq!(Viewport::new(f64::NAN, f64::INFINITY, 0.0, 0.0).top, 0.0);
    }
}
