//! Porte de `src/vs/editor/common/viewModel/viewModelImpl.ts`, linhas 574 a 1299: os métodos públicos da classe
//! `ViewModel` depois do construtor (a parte A, linhas 51 a 573, está em `view_model_impl.rs`).
//!
//! Representação:
//! - O que o `ICursorSimpleModel` declara (`getLineCount`, `getLineContent`, `getLineMinColumn`...,
//!   `normalizePosition`, `getLineIndentColumn`) é o `impl CursorSimpleModel for ViewModel<T>`, e os métodos só
//!   existem ali (não há inerente de mesmo nome). O que o `IViewModel` declara é o `impl IViewModel`, inclusive
//!   `addViewEventHandler`, `removeViewEventHandler`, `setHasFocus`, `setHasWidgetFocus`, `onCompositionStart`,
//!   `onCompositionEnd`, `visibleLinesStabilized` e `createLineBreaksComputer`, que a parte A deixou para cá. O que
//!   só a classe tem (`setHiddenAreas`, `saveState`, `executeEdits`...) é `impl` inerente.
//! - `IViewState` (`editorCommon.ts`) não tinha porte: está aqui, com `firstPosition` opcional (o campo
//!   `undefined` de um estado salvo por versões antigas), assim como o resultado de `reduceRestoreState`
//!   ([`ReducedRestoreState`]).
//! - `_toModelVisibleRanges`, `_invalidateDecorationsColorCache` e `_emitViewEvent` são `pub(crate)`, porque a
//!   parte A os chama. `_withViewEventsCollector` e `_executeCursorEdit` são privados.
//! - `source: string | null | undefined` é `Option<&str>`; `reason = CursorChangeReason.NotSet` de `setSelections`
//!   é parâmetro obrigatório; texto digitado e colado é `&[u16]` (UTF-16, como o controlador de cursores).
//! - `getEditorOption(id)` é [`ViewModel::get_editor_option`] (parte A) e `IViewModel::computed_editor_options`.
//! - `_invalidateDecorationsColorCache`: no original, `ModelDecorationOverviewRulerOptions` e
//!   `ModelDecorationMinimapOptions` guardam a cor resolvida em cache e a função zera esse cache. As opções do
//!   porte (`text_model.rs`) não guardam a cor resolvida: [`overview_ruler_color`] resolve a cada chamada com o
//!   tema vigente, então não há cache a invalidar e a função é vazia (o resultado é o mesmo de um cache sempre
//!   invalidado a cada troca de tema).
//! - `getAllOverviewRulerDecorations(theme)`: a cor de uma decoração da régua usa `getColor(theme)` do original
//!   (cor escura para tema que não é `light`, inclusive `hcLight`; `ThemeColor` consulta o tema; sem cor, texto
//!   vazio), em [`overview_ruler_color`].
//! - `tokenizeLineToHTML` (`languages/textToHtmlTokenizer.ts`), que `_getHTMLToCopy` usa, ainda não tinha porte:
//!   está aqui, como função privada ([`tokenize_line_to_html`]). O HTML é montado em unidades UTF-16 (o original
//!   acrescenta `String.fromCharCode` unidade por unidade) e convertido para `String` no fim, com a perda de
//!   U+FFFD num surrogate isolado. O `colorMap` do `TokenizationRegistry` já guarda texto CSS (ver
//!   `tokenization_registry.rs`), então `_getColorMap` só troca o índice 0 por `#000000`; uma cor ausente sai como
//!   o texto `undefined`, como a interpolação do original.
//! - `getRichTextToCopy` acha `platform.isWindows` por [`Platform::detect`].
//! - `batchEvents(callback)` e `changeWhitespace(callback)` recebem `&mut dyn FnMut`, como o `IViewModel`.
//! - `getTabSize` é privado; `_getTextDirection` é a função livre [`text_direction_of`] (não usa o `this`).

use std::rc::Rc;

use je_base::JsString;
use je_base::platform::Platform;
use je_base::strings;
use je_theme::ThemeType;

use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::effective_options::EFFECTIVE_ALLOW_VARIABLE_FONTS;
use crate::config::editor_options::layout_info::LAYOUT_INFO;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::registry::CURSOR_SURROUNDING_LINES;
use crate::config::editor_options::sticky_scroll::STICKY_SCROLL;
use crate::config::editor_options::ComputedEditorOptions;
use crate::config::font_info::editor_font_defaults;
use crate::coordinates_converter::ICoordinatesConverter;
use crate::cursor::SavedCursorState;
use crate::cursor_common::{
    ColumnSelectData, CursorSimpleModel, CursorState, EditOperationType, PartialCursorState,
};
use crate::edit_operation::SingleEditOperation;
use crate::editor_common::{Command, ScrollType};
use crate::editor_theme::EditorTheme;
use crate::encoded_token_attributes::color_id;
use crate::glyph_lanes_model::GlyphMarginLanesModel;
use crate::inline_decorations::InlineDecoration;
use crate::line_tokens::IViewLineTokens;
use crate::model::{
    DecorationColor, EndOfLinePreference, Eol, PositionAffinity, TextDirection, ValidEditOperation,
};
use crate::model_line_projection_data::{InjectedText, LineBreaksComputer, LineBreaksComputerContext};
use crate::modes_registry::PLAINTEXT_LANGUAGE_ID;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::{ModelDecorationOverviewRulerOptions, TextModel};
use crate::text_model_edit_source::TextModelEditSource;
use crate::text_model_guides::{ActiveIndentGuideInfo, BracketGuideOptions, IndentGuide};
use crate::tokenization_registry::tokenization_registry;
use crate::view_events::{
    ViewCompositionEndEvent, ViewCompositionStartEvent, ViewEvent, ViewEventHandler, ViewFocusChangedEvent,
    ViewRevealRangeRequestEvent, ViewZonesChangedEvent,
};
use crate::view_layout::ViewLayout;
use crate::view_model_decorations::{filter_font_decorations, filter_validation_decorations};
use crate::view_model_event_dispatcher::{
    FocusChangedEvent, HiddenAreasChangedEvent, OutgoingViewModelEvent, ReadOnlyEditAttemptEvent,
    ViewModelEventsCollector, ViewZonesChangedEvent as OutgoingViewZonesChangedEvent, WidgetFocusChangedEvent,
};
use crate::view_model_impl::{HiddenAreasSource, ViewEventsEmission, ViewModel};
use crate::view_model_impl_support::{IBatchableTarget, OverviewRulerDecorations};
use crate::view_model_lines::compare_ranges_using_starts;
use crate::view_model_types::{
    CursorChangeReason, IPartialViewLinesViewportData, IViewLayout, IViewModel, IWhitespaceChangeAccessor,
    MinimapLinesRenderingData, OverviewRulerDecorationsGroup, PlainTextSource, PlainTextToCopy, RichTextToCopy,
    VerticalRevealType, ViewLineData, ViewLineRenderingData, ViewModelDecoration,
};

/// `IViewState` (`editorCommon.ts`): o estado serializável da visão. `scrollTop` e `scrollTopWithoutViewZones`
/// são "escritos por versões anteriores"; `firstPosition` ausente marca um estado dessas versões.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IViewState {
    pub scroll_top: Option<f64>,
    pub scroll_top_without_view_zones: Option<f64>,
    pub scroll_left: f64,
    pub first_position: Option<Position>,
    pub first_position_delta_top: f64,
}

/// O resultado de `reduceRestoreState`: `{ scrollLeft, scrollTop }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReducedRestoreState {
    pub scroll_left: f64,
    pub scroll_top: f64,
}

/// `Math.round`: arredonda para cima nos meios.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// Os `ViewModelDecoration` que o computador de decorações devolve em `Rc`, como valores do `IViewModel`.
fn to_owned_decorations(decorations: &[Rc<ViewModelDecoration>]) -> Vec<ViewModelDecoration> {
    decorations.iter().map(|decoration| (**decoration).clone()).collect()
}

/// `_getTextDirection`.
fn text_direction_of(line_number: i32, decorations: &[Rc<ViewModelDecoration>]) -> TextDirection {
    let mut rtl_count = 0;

    for decoration in decorations {
        let range = decoration.range;
        if range.start_line_number > line_number || range.end_line_number < line_number {
            continue;
        }
        match decoration.options.text_direction {
            Some(TextDirection::Rtl) => rtl_count += 1,
            Some(TextDirection::Ltr) => rtl_count -= 1,
            None => {}
        }
    }

    if rtl_count > 0 { TextDirection::Rtl } else { TextDirection::Ltr }
}

/// `ThemeColor | string` de uma opção da régua, resolvido pelo tema (`_resolveColor`): o texto é a própria cor, o
/// `ThemeColor` consulta o tema e sem cor o resultado é vazio.
fn resolve_decoration_color(color: &DecorationColor, theme: &EditorTheme) -> String {
    match color {
        DecorationColor::Color(value) => value.clone(),
        DecorationColor::Theme(id) => theme.get_color(id).map_or_else(String::new, |color| color.to_string()),
    }
}

/// `ModelDecorationOverviewRulerOptions.getColor(theme)`: a cor escura vale para todo tema que não é `light`.
fn overview_ruler_color(options: &ModelDecorationOverviewRulerOptions, theme: &EditorTheme) -> String {
    let has_dark_color = match &options.dark_color {
        DecorationColor::Color(value) => !value.is_empty(),
        DecorationColor::Theme(_) => true,
    };
    if theme.theme_type() != ThemeType::Light && has_dark_color {
        resolve_decoration_color(&options.dark_color, theme)
    } else {
        resolve_decoration_color(&options.color, theme)
    }
}

/// `_getColorMap`: o mapa de cores do registro de tokenização em texto CSS, com `#000000` no índice 0.
fn get_color_map() -> Vec<String> {
    let mut result = vec!["#000000".to_owned()];
    if let Some(color_map) = tokenization_registry().get_color_map() {
        result.extend(color_map.into_iter().skip(1));
    }
    result
}

/// A família de fonte do HTML da cópia rica (o miolo de `getRichTextToCopy`).
fn rich_text_font_family(font_family: &str) -> String {
    let default_font_family = editor_font_defaults().font_family;
    let has_bad_chars = font_family.contains([':', ';', '\\', '/', '<', '>']);
    if has_bad_chars || font_family == default_font_family {
        return default_font_family.to_owned();
    }
    let mut font_family = font_family.replace('"', "'");
    let has_quotes_or_is_list = font_family.contains([',', '\'']);
    if !has_quotes_or_is_list {
        let needs_quotes = font_family.contains(['+', ' ']);
        if needs_quotes {
            font_family = format!("'{font_family}'");
        }
    }
    format!("{font_family}, {default_font_family}")
}

const CHAR_TAB: u16 = 9;
const CHAR_CARRIAGE_RETURN: u16 = 13;
const CHAR_SPACE: u16 = 32;
const CHAR_AMPERSAND: u16 = 38;
const CHAR_LESS_THAN: u16 = 60;
const CHAR_GREATER_THAN: u16 = 62;
const CHAR_NEXT_LINE: u16 = 0x85;
const CHAR_LINE_SEPARATOR: u16 = 0x2028;
const CHAR_PARAGRAPH_SEPARATOR: u16 = 0x2029;
const CHAR_UTF8_BOM: u16 = 0xFEFF;
const CHAR_REPLACEMENT: u16 = 0xFFFD;

fn push_text(out: &mut Vec<u16>, text: &str) {
    out.extend(text.encode_utf16());
}

/// Um espaço do `tokenizeLineToHTML`: `&#160;` se o anterior também foi um espaço (e `useNbsp`), senão ` `.
fn push_space(out: &mut Vec<u16>, prev_is_space: &mut bool, use_nbsp: bool) {
    if use_nbsp && *prev_is_space {
        push_text(out, "&#160;");
        *prev_is_space = false;
    } else {
        out.push(CHAR_SPACE);
        *prev_is_space = true;
    }
}

/// `tokenizeLineToHTML(text, viewLineTokens, colorMap, startOffset, endOffset, tabSize, useNbsp)`.
fn tokenize_line_to_html(
    text: &[u16],
    view_line_tokens: &dyn IViewLineTokens,
    color_map: &[String],
    start_offset: i32,
    end_offset: i32,
    tab_size: i32,
    use_nbsp: bool,
) -> String {
    let mut result: Vec<u16> = Vec::new();
    push_text(&mut result, "<div>");
    let mut char_index: i32 = 0;
    let mut width: i32 = 0;

    let mut prev_is_space = true;

    for token_index in 0..view_line_tokens.get_count() {
        let token_end_index = view_line_tokens.get_end_offset(token_index);
        let mut part_content: Vec<u16> = Vec::new();

        while char_index < token_end_index && char_index < end_offset {
            let char_code = text[char_index as usize];
            let is_tab = char_code == CHAR_TAB;

            width += if strings::is_full_width_character(char_code) {
                2
            } else if is_tab {
                0
            } else {
                1
            };

            if char_index < start_offset {
                if is_tab {
                    let remainder = width % tab_size;
                    width += if remainder == 0 { tab_size } else { tab_size - remainder };
                }
                char_index += 1;
                continue;
            }

            match char_code {
                CHAR_TAB => {
                    let remainder = width % tab_size;
                    let insert_spaces_count = if remainder == 0 { tab_size } else { tab_size - remainder };
                    width += insert_spaces_count;
                    for _ in 0..insert_spaces_count {
                        push_space(&mut part_content, &mut prev_is_space, use_nbsp);
                    }
                }
                CHAR_SPACE => push_space(&mut part_content, &mut prev_is_space, use_nbsp),
                _ => {
                    match char_code {
                        CHAR_LESS_THAN => push_text(&mut part_content, "&lt;"),
                        CHAR_GREATER_THAN => push_text(&mut part_content, "&gt;"),
                        CHAR_AMPERSAND => push_text(&mut part_content, "&amp;"),
                        0 => push_text(&mut part_content, "&#00;"),
                        CHAR_UTF8_BOM | CHAR_LINE_SEPARATOR | CHAR_PARAGRAPH_SEPARATOR | CHAR_NEXT_LINE => {
                            part_content.push(CHAR_REPLACEMENT);
                        }
                        // zero width space, because carriage return would introduce a line break
                        CHAR_CARRIAGE_RETURN => push_text(&mut part_content, "&#8203"),
                        other => part_content.push(other),
                    }
                    prev_is_space = false;
                }
            }
            char_index += 1;
        }

        if token_end_index <= start_offset {
            continue;
        }

        push_text(&mut result, "<span style=\"");
        push_text(&mut result, &view_line_tokens.get_inline_style(token_index, color_map));
        push_text(&mut result, "\">");
        result.extend_from_slice(&part_content);
        push_text(&mut result, "</span>");

        if token_end_index > end_offset || char_index >= end_offset || start_offset >= end_offset {
            break;
        }
    }

    push_text(&mut result, "</div>");
    String::from_utf16_lossy(&result)
}

/// O que o `ICursorSimpleModel` pede do modelo de visão: as linhas da visão.
impl<T: IBatchableTarget + 'static> CursorSimpleModel for ViewModel<T> {
    fn get_line_count(&self) -> i32 {
        self.lines.get_view_line_count()
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        self.lines.get_view_line_content(line_number)
    }

    fn get_line_min_column(&self, line_number: i32) -> i32 {
        self.lines.get_view_line_min_column(line_number)
    }

    fn get_line_max_column(&self, line_number: i32) -> i32 {
        self.lines.get_view_line_max_column(line_number)
    }

    fn get_line_first_non_whitespace_column(&self, line_number: i32) -> i32 {
        match strings::first_non_whitespace_index(self.get_line_content(line_number).units()) {
            None => 0,
            Some(result) => result as i32 + 1,
        }
    }

    fn get_line_last_non_whitespace_column(&self, line_number: i32) -> i32 {
        let content = self.get_line_content(line_number);
        match strings::last_non_whitespace_index(content.units(), content.len() as i64 - 1) {
            None => 0,
            Some(result) => result as i32 + 2,
        }
    }

    fn normalize_position(&self, position: Position, affinity: PositionAffinity) -> Position {
        self.lines.normalize_position(position, affinity)
    }

    /// Gets the column at which indentation stops at a given line.
    fn get_line_indent_column(&self, line_number: i32) -> i32 {
        self.lines.get_line_indent_column(line_number)
    }
}

impl<T: IBatchableTarget + 'static> IViewModel for ViewModel<T> {
    type EventHandler = dyn ViewEventHandler;
    type EditorTheme = EditorTheme;
    type GlyphLanes = GlyphMarginLanesModel;

    fn model(&self) -> Rc<TextModel> {
        self.model.clone()
    }

    fn coordinates_converter(&self) -> Rc<dyn ICoordinatesConverter> {
        self.coordinates_converter.clone()
    }

    fn view_layout(&self) -> Rc<dyn IViewLayout> {
        let view_layout: Rc<ViewLayout> = self.layout().clone();
        view_layout
    }

    fn cursor_config(&self) -> Rc<crate::cursor_common::CursorConfiguration> {
        self.cursor_config.borrow().clone()
    }

    fn glyph_lanes(&self) -> Rc<GlyphMarginLanesModel> {
        self.glyph_lanes.clone()
    }

    fn add_view_event_handler(&self, event_handler: Rc<dyn ViewEventHandler>) {
        self.event_dispatcher.add_view_event_handler(event_handler);
    }

    fn remove_view_event_handler(&self, event_handler: &Rc<dyn ViewEventHandler>) {
        self.event_dispatcher.remove_view_event_handler(event_handler);
    }

    fn computed_editor_options(&self) -> &ComputedEditorOptions {
        self.configuration.options()
    }

    /// Gives a hint that a lot of requests are about to come in for these line numbers.
    fn set_viewport(&self, start_line_number: i32, _end_line_number: i32, _centered_line_number: i32) {
        self.viewport_start.borrow_mut().update(self, start_line_number);
    }

    fn visible_lines_stabilized(&self) {
        self.set_attached_view_visible_lines(true);
    }

    fn set_has_focus(&self, has_focus: bool) {
        self.has_focus.set(has_focus);
        self.cursor().set_has_focus(has_focus);
        self.event_dispatcher.emit_single_view_event(ViewEvent::FocusChanged(ViewFocusChangedEvent::new(has_focus)));
        self.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::FocusChanged(FocusChangedEvent::new(
            !has_focus, has_focus,
        )));
    }

    fn set_has_widget_focus(&self, has_widget_focus: bool) {
        self.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::WidgetFocusChanged(
            WidgetFocusChangedEvent::new(!has_widget_focus, has_widget_focus),
        ));
    }

    fn on_composition_start(&self) {
        self.event_dispatcher.emit_single_view_event(ViewEvent::CompositionStart(ViewCompositionStartEvent));
    }

    fn on_composition_end(&self) {
        self.event_dispatcher.emit_single_view_event(ViewEvent::CompositionEnd(ViewCompositionEndEvent));
    }

    fn get_font_size_at_position(&self, position: Position) -> Option<String> {
        let options = self.configuration.options();
        if !*options.get(EFFECTIVE_ALLOW_VARIABLE_FONTS) {
            return None;
        }
        let font_decorations =
            self.model.get_font_decorations_in_range(&Range::from_positions(position, position), self.editor_id);
        let mut font_size = format!("{}px", options.get(FONT_INFO).font_size);
        for font_decoration in &font_decorations {
            if let Some(decoration_font_size) = font_decoration.options.font_size.as_ref().filter(|size| !size.is_empty()) {
                font_size = decoration_font_size.clone();
                break;
            }
        }
        Some(font_size)
    }

    fn get_minimap_decorations_in_range(&self, range: Range) -> Vec<ViewModelDecoration> {
        to_owned_decorations(&self.decorations.get_minimap_decorations_in_range(range))
    }

    fn get_decorations_in_viewport(&self, visible_range: Range) -> Vec<ViewModelDecoration> {
        to_owned_decorations(&self.decorations.get_decorations_viewport_data(visible_range).decorations)
    }

    fn get_text_direction(&self, line_number: i32) -> TextDirection {
        let decorations_collection = self.decorations.get_decorations_on_line(line_number, false, false);
        text_direction_of(line_number, &decorations_collection.decorations)
    }

    fn get_viewport_view_line_rendering_data(&self, visible_range: Range, line_number: i32) -> ViewLineRenderingData {
        let viewport_decorations_collection = self.decorations.get_decorations_viewport_data(visible_range);
        let relative_line_number = (line_number - visible_range.start_line_number) as usize;
        let inline_decorations = viewport_decorations_collection.inline_decorations[relative_line_number].clone();
        let has_variable_fonts = viewport_decorations_collection.has_variable_fonts[relative_line_number];
        self.build_view_line_rendering_data(
            line_number,
            inline_decorations,
            has_variable_fonts,
            &viewport_decorations_collection.decorations,
        )
    }

    fn get_view_line_rendering_data(&self, line_number: i32) -> ViewLineRenderingData {
        let decorations_collection = self.decorations.get_decorations_on_line(line_number, false, false);
        self.build_view_line_rendering_data(
            line_number,
            decorations_collection.inline_decorations[0].clone(),
            decorations_collection.has_variable_fonts[0],
            &decorations_collection.decorations,
        )
    }

    fn get_view_line_continues_with_wrapped_line(&self, line_number: i32) -> bool {
        self.lines.get_view_line_continues_with_wrapped_line(line_number)
    }

    fn get_view_line_data(&self, line_number: i32) -> ViewLineData {
        self.lines.get_view_line_data(line_number)
    }

    fn get_minimap_lines_rendering_data(
        &self,
        start_line_number: i32,
        end_line_number: i32,
        needed: &[bool],
    ) -> MinimapLinesRenderingData {
        let result = self.lines.get_view_lines_data(start_line_number, end_line_number, needed);
        MinimapLinesRenderingData::new(self.get_tab_size(), result)
    }

    fn get_completely_visible_view_range(&self) -> Range {
        self.completely_visible_view_range(&self.layout().get_lines_viewport_data())
    }

    fn get_completely_visible_view_range_at_scroll_top(&self, scroll_top: f64) -> Range {
        self.completely_visible_view_range(&self.layout().get_lines_viewport_data_at_scroll_top(scroll_top))
    }

    /// Applies `cursorSurroundingLines` and `stickyScroll` padding to the given view range.
    fn get_view_range_with_cursor_padding(&self, view_range: Range) -> Range {
        let options = self.configuration.options();
        let cursor_surrounding_lines = *options.get(CURSOR_SURROUNDING_LINES);
        let sticky_scroll = options.get(STICKY_SCROLL);

        let mut start_line_number = f64::from(view_range.start_line_number);
        let mut end_line_number = f64::from(view_range.end_line_number);
        let padding = cursor_surrounding_lines
            .max(if sticky_scroll.enabled { sticky_scroll.max_line_count } else { 0.0 })
            .min(((end_line_number - start_line_number + 1.0) / 2.0).floor());

        start_line_number += padding;
        end_line_number -= 0f64.max(padding - 1.0);

        if padding == 0.0 || start_line_number > end_line_number {
            return view_range;
        }

        let (start_line_number, end_line_number) = (start_line_number as i32, end_line_number as i32);
        Range::new(
            start_line_number,
            self.get_line_min_column(start_line_number),
            end_line_number,
            self.get_line_max_column(end_line_number),
        )
    }

    fn get_hidden_areas(&self) -> Vec<Range> {
        self.lines.get_hidden_areas()
    }

    fn get_line_length(&self, line_number: i32) -> usize {
        self.lines.get_view_line_length(line_number)
    }

    fn get_active_indent_guide(&self, line_number: i32, min_line_number: i32, max_line_number: i32) -> ActiveIndentGuideInfo {
        self.lines.get_active_indent_guide(line_number, min_line_number, max_line_number)
    }

    fn get_lines_indent_guides(&self, start_line_number: i32, end_line_number: i32) -> Vec<i32> {
        self.lines.get_view_lines_indent_guides(start_line_number, end_line_number)
    }

    fn get_bracket_guides_in_range_by_line(
        &self,
        start_line_number: i32,
        end_line_number: i32,
        active_position: Option<Position>,
        options: BracketGuideOptions,
    ) -> Vec<Vec<IndentGuide>> {
        self.lines.get_view_lines_bracket_guides(start_line_number, end_line_number, active_position, options)
    }

    fn get_all_overview_ruler_decorations(&self, theme: &EditorTheme) -> Vec<OverviewRulerDecorationsGroup> {
        let options = self.configuration.options();
        let decorations = self.model.get_overview_ruler_decorations(
            self.editor_id,
            filter_validation_decorations(options),
            filter_font_decorations(options),
        );
        let mut result = OverviewRulerDecorations::default();
        for decoration in &decorations {
            let decoration_options = &decoration.options;
            let Some(opts) = decoration_options.overview_ruler.as_ref() else {
                continue;
            };
            let lane = opts.position as i32;
            if lane == 0 {
                continue;
            }
            let color = overview_ruler_color(opts, theme);
            let view_start_line_number = self
                .coordinates_converter
                .get_view_line_number_of_model_position(decoration.range.start_line_number, decoration.range.start_column);
            let view_end_line_number = self
                .coordinates_converter
                .get_view_line_number_of_model_position(decoration.range.end_line_number, decoration.range.end_column);

            result.accept(&color, decoration_options.z_index, view_start_line_number, view_end_line_number, lane);
        }
        result.as_array
    }

    fn get_value_in_range(&self, range: Range, eol: EndOfLinePreference) -> JsString {
        let model_range = self.coordinates_converter.convert_view_range_to_model_range(range);
        self.model.get_value_in_range_with(&model_range, eol)
    }

    fn get_value_length_in_range(&self, range: Range, eol: EndOfLinePreference) -> usize {
        let model_range = self.coordinates_converter.convert_view_range_to_model_range(range);
        self.model.get_value_length_in_range_with(&model_range, eol)
    }

    fn modify_position(&self, position: Position, offset: i32) -> Position {
        let model_position = self.coordinates_converter.convert_view_position_to_model_position(position);
        let result_model_position = self.model.modify_position(model_position, i64::from(offset));
        self.coordinates_converter.convert_model_position_to_view_position(result_model_position, None, None, None)
    }

    fn get_injected_text_at(&self, view_position: Position) -> Option<InjectedText> {
        self.lines.get_injected_text_at(view_position)
    }

    fn deduce_model_position_relative_to_view_position(
        &self,
        view_anchor_position: Position,
        delta_offset: i32,
        line_feed_cnt: i32,
    ) -> Position {
        let model_anchor = self.coordinates_converter.convert_view_position_to_model_position(view_anchor_position);
        let mut delta_offset = i64::from(delta_offset);
        if self.model.get_eol().len() == 2 {
            // This model uses CRLF, so the delta must take that into account
            if delta_offset < 0 {
                delta_offset -= i64::from(line_feed_cnt);
            } else {
                delta_offset += i64::from(line_feed_cnt);
            }
        }

        let model_anchor_offset = self.model.get_offset_at(model_anchor) as i64;
        let result_offset = model_anchor_offset + delta_offset;
        self.model.get_position_at(result_offset)
    }

    fn get_plain_text_to_copy(&self, model_ranges: &[Range], empty_selection_clipboard: bool, force_crlf: bool) -> PlainTextToCopy {
        let new_line_character: &[u16] = if force_crlf { Eol::CrLf.as_units() } else { self.model.get_eol().as_units() };

        let mut model_ranges = model_ranges.to_vec();
        model_ranges.sort_by(compare_ranges_using_starts);

        let mut has_empty_range = false;
        let mut has_non_empty_range = false;
        for range in &model_ranges {
            if range.is_empty() {
                has_empty_range = true;
            } else {
                has_non_empty_range = true;
            }
        }

        if !has_non_empty_range && !empty_selection_clipboard {
            // all ranges are empty
            return PlainTextToCopy { source_ranges: Vec::new(), source_text: PlainTextSource::Text(JsString::new()) };
        }

        let eol = if force_crlf { EndOfLinePreference::CrLf } else { EndOfLinePreference::TextDefined };
        let mut ranges: Vec<Range> = Vec::new();
        let mut result: Vec<JsString> = Vec::new();
        let mut push_range = |model_range: Range, append: &[u16]| {
            ranges.push(model_range);
            let mut value = self.model.get_value_in_range_with(&model_range, eol);
            value.push_units(append);
            result.push(value);
        };

        if has_empty_range && empty_selection_clipboard {
            // some (maybe all) empty selections
            let mut prev_model_line_number = 0;
            for model_range in &model_ranges {
                let model_line_number = model_range.start_line_number;
                if model_range.is_empty() {
                    if model_line_number != prev_model_line_number {
                        push_range(
                            Range::new(
                                model_line_number,
                                self.model.get_line_min_column(model_line_number),
                                model_line_number,
                                self.model.get_line_max_column(model_line_number),
                            ),
                            new_line_character,
                        );
                    }
                } else {
                    push_range(*model_range, &[]);
                }
                prev_model_line_number = model_line_number;
            }
        } else {
            for model_range in &model_ranges {
                if !model_range.is_empty() {
                    push_range(*model_range, &[]);
                }
            }
        }

        let source_text = if result.len() == 1 { PlainTextSource::Text(result.remove(0)) } else { PlainTextSource::Lines(result) };
        PlainTextToCopy { source_ranges: ranges, source_text }
    }

    fn get_rich_text_to_copy(&self, model_ranges: &[Range], empty_selection_clipboard: bool) -> Option<RichTextToCopy> {
        let language_id = self.model.get_language_id();
        if language_id == PLAINTEXT_LANGUAGE_ID {
            return None;
        }

        if model_ranges.len() != 1 {
            // no multiple selection support at this time
            return None;
        }

        let mut range = model_ranges[0];
        if range.is_empty() {
            if !empty_selection_clipboard {
                // nothing to copy
                return None;
            }
            let line_number = range.start_line_number;
            range = Range::new(
                line_number,
                self.model.get_line_min_column(line_number),
                line_number,
                self.model.get_line_max_column(line_number),
            );
        }

        let font_info = self.configuration.options().get(FONT_INFO);
        let color_map = get_color_map();
        let font_family = rich_text_font_family(&font_info.font_family);
        let color_of = |id: u32| color_map.get(id as usize).map_or("undefined", String::as_str);

        Some(RichTextToCopy {
            mode: language_id,
            html: format!(
                "<div style=\"color: {};background-color: {};font-family: {};font-weight: {};font-size: {}px;line-height: {}px;white-space: pre;\">{}</div>",
                color_of(color_id::DEFAULT_FOREGROUND),
                color_of(color_id::DEFAULT_BACKGROUND),
                font_family,
                font_info.font_weight,
                font_info.font_size,
                font_info.line_height,
                self.get_html_to_copy(range, &color_map),
            ),
        })
    }

    // #region cursor

    fn get_primary_cursor_state(&self) -> CursorState {
        self.cursor().get_primary_cursor_state()
    }

    fn get_last_added_cursor_index(&self) -> usize {
        self.cursor().get_last_added_cursor_index()
    }

    fn get_cursor_states(&self) -> Vec<CursorState> {
        self.cursor().get_cursor_states()
    }

    fn set_cursor_states(&self, source: Option<&str>, reason: CursorChangeReason, states: Option<Vec<PartialCursorState>>) -> bool {
        self.with_view_events_collector(|events_collector| self.cursor().set_states(events_collector, source, reason, states))
    }

    fn get_cursor_column_select_data(&self) -> ColumnSelectData {
        self.cursor().get_cursor_column_select_data()
    }

    fn get_cursor_auto_closed_characters(&self) -> Vec<Range> {
        self.cursor().get_auto_closed_characters()
    }

    fn set_cursor_column_select_data(&self, column_select_data: ColumnSelectData) {
        self.cursor().set_cursor_column_select_data(column_select_data);
    }

    fn get_prev_edit_operation_type(&self) -> EditOperationType {
        self.cursor().get_prev_edit_operation_type()
    }

    fn set_prev_edit_operation_type(&self, r#type: EditOperationType) {
        self.cursor().set_prev_edit_operation_type(r#type);
    }

    fn reveal_all_cursors(&self, source: Option<&str>, reveal_horizontal: bool, minimal_reveal: Option<bool>) {
        self.with_view_events_collector(|events_collector| {
            self.cursor().reveal_all(
                events_collector,
                source,
                minimal_reveal.unwrap_or(false),
                VerticalRevealType::Simple,
                reveal_horizontal,
                ScrollType::Smooth,
            );
        });
    }

    fn reveal_primary_cursor(&self, source: Option<&str>, reveal_horizontal: bool, minimal_reveal: Option<bool>) {
        self.with_view_events_collector(|events_collector| {
            self.cursor().reveal_primary(
                events_collector,
                source,
                minimal_reveal.unwrap_or(false),
                VerticalRevealType::Simple,
                reveal_horizontal,
                ScrollType::Smooth,
            );
        });
    }

    fn reveal_top_most_cursor(&self, source: Option<&str>) {
        let view_position = self.cursor().get_top_most_view_position();
        let view_range = Range::new(view_position.line_number, view_position.column, view_position.line_number, view_position.column);
        self.reveal_range(source, true, view_range, VerticalRevealType::Simple, ScrollType::Smooth);
    }

    fn reveal_bottom_most_cursor(&self, source: Option<&str>) {
        let view_position = self.cursor().get_bottom_most_view_position();
        let view_range = Range::new(view_position.line_number, view_position.column, view_position.line_number, view_position.column);
        self.reveal_range(source, true, view_range, VerticalRevealType::Simple, ScrollType::Smooth);
    }

    fn reveal_range(
        &self,
        source: Option<&str>,
        reveal_horizontal: bool,
        view_range: Range,
        vertical_type: VerticalRevealType,
        scroll_type: ScrollType,
    ) {
        self.with_view_events_collector(|events_collector| {
            events_collector.emit_view_event(ViewEvent::RevealRangeRequest(ViewRevealRangeRequestEvent::new(
                source.map(str::to_owned),
                false,
                Some(view_range),
                None,
                vertical_type,
                reveal_horizontal,
                scroll_type,
            )));
        });
    }

    // #endregion

    // #region viewLayout

    fn change_whitespace(&self, callback: &mut dyn FnMut(&mut dyn IWhitespaceChangeAccessor)) {
        let had_a_change = self.layout().change_whitespace(|accessor| callback(accessor));
        if had_a_change {
            self.event_dispatcher.emit_single_view_event(ViewEvent::ZonesChanged(ViewZonesChangedEvent));
            self.event_dispatcher
                .emit_outgoing_event(OutgoingViewModelEvent::ViewZonesChanged(OutgoingViewZonesChangedEvent::new()));
        }
    }

    // #endregion

    fn create_line_breaks_computer<'a>(
        &'a self,
        context: Option<&'a dyn LineBreaksComputerContext>,
    ) -> Box<dyn LineBreaksComputer + 'a> {
        self.lines.create_line_breaks_computer(context)
    }

    fn batch_events(&self, callback: &mut dyn FnMut()) {
        self.with_view_events_collector(|_| callback());
    }
}

impl<T: IBatchableTarget + 'static> ViewModel<T> {
    /// `_getViewLineRenderingData`.
    fn build_view_line_rendering_data(
        &self,
        line_number: i32,
        mut inline_decorations: Vec<InlineDecoration>,
        has_variable_fonts: bool,
        decorations: &[Rc<ViewModelDecoration>],
    ) -> ViewLineRenderingData {
        let might_contain_rtl = self.model.might_contain_rtl();
        let might_contain_non_basic_ascii = self.model.might_contain_non_basic_ascii();
        let tab_size = self.get_tab_size();
        let line_data = self.lines.get_view_line_data(line_number);

        if let Some(line_inline_decorations) = &line_data.inline_decorations {
            inline_decorations.extend(line_inline_decorations.iter().cloned());
        }

        ViewLineRenderingData::new(
            line_data.min_column,
            line_data.max_column,
            line_data.content,
            line_data.continues_with_wrapped_line,
            might_contain_rtl,
            might_contain_non_basic_ascii,
            line_data.tokens,
            inline_decorations,
            tab_size,
            line_data.start_visible_column,
            text_direction_of(line_number, decorations),
            has_variable_fonts,
        )
    }

    /// O intervalo completamente visível de uns dados parciais do viewport (a conta de
    /// `getCompletelyVisibleViewRange` e de `getCompletelyVisibleViewRangeAtScrollTop`).
    fn completely_visible_view_range(&self, partial_data: &IPartialViewLinesViewportData) -> Range {
        let start_view_line_number = partial_data.completely_visible_start_line_number;
        let end_view_line_number = partial_data.completely_visible_end_line_number;

        Range::new(
            start_view_line_number,
            self.get_line_min_column(start_view_line_number),
            end_view_line_number,
            self.get_line_max_column(end_view_line_number),
        )
    }

    /// `getTabSize`.
    fn get_tab_size(&self) -> i32 {
        self.model.get_options().tab_size
    }

    /// `_getHTMLToCopy`.
    fn get_html_to_copy(&self, model_range: Range, color_map: &[String]) -> String {
        let start_line_number = model_range.start_line_number;
        let start_column = model_range.start_column;
        let end_line_number = model_range.end_line_number;
        let end_column = model_range.end_column;

        let tab_size = self.get_tab_size();
        let use_nbsp = Platform::detect().is_windows;

        let mut result = String::new();

        for line_number in start_line_number..=end_line_number {
            let line_tokens = self.model.tokenization().get_line_tokens(line_number);
            let line_content = line_tokens.get_line_content_ref();
            let start_offset = if line_number == start_line_number { start_column - 1 } else { 0 };
            let end_offset = if line_number == end_line_number { end_column - 1 } else { line_content.len() as i32 };

            if line_content.is_empty() {
                result.push_str("<br>");
            } else {
                result.push_str(&tokenize_line_to_html(
                    line_content.units(),
                    &line_tokens.inflate(),
                    color_map,
                    start_offset,
                    end_offset,
                    tab_size,
                    use_nbsp,
                ));
            }
        }

        result
    }

    /// `_toModelVisibleRanges`.
    pub(crate) fn to_model_visible_ranges(&self, visible_view_range: Range) -> Vec<Range> {
        let visible_range = self.coordinates_converter.convert_view_range_to_model_range(visible_view_range);
        let hidden_areas = self.lines.get_hidden_areas();

        if hidden_areas.is_empty() {
            return vec![visible_range];
        }

        let mut result: Vec<Range> = Vec::new();
        let mut start_line_number = visible_range.start_line_number;
        let mut start_column = visible_range.start_column;
        let end_line_number = visible_range.end_line_number;
        let end_column = visible_range.end_column;
        for hidden_area in &hidden_areas {
            let hidden_start_line_number = hidden_area.start_line_number;
            let hidden_end_line_number = hidden_area.end_line_number;

            if hidden_end_line_number < start_line_number {
                continue;
            }
            if hidden_start_line_number > end_line_number {
                continue;
            }

            if start_line_number < hidden_start_line_number {
                result.push(Range::new(
                    start_line_number,
                    start_column,
                    hidden_start_line_number - 1,
                    self.model.get_line_max_column(hidden_start_line_number - 1),
                ));
            }
            start_line_number = hidden_end_line_number + 1;
            start_column = 1;
        }

        if start_line_number < end_line_number || (start_line_number == end_line_number && start_column < end_column) {
            result.push(Range::new(start_line_number, start_column, end_line_number, end_column));
        }

        result
    }

    /// `_invalidateDecorationsColorCache`: sem cache de cor nas opções das decorações (ver "Representação" no
    /// topo do módulo), não há o que invalidar.
    pub(crate) fn invalidate_decorations_color_cache(&self) {}

    /// `_emitViewEvent`: o `try { beginEmitViewEvents(); callback } finally { endEmitViewEvents() }`.
    pub(crate) fn emit_view_event<R>(&self, callback: impl FnOnce(&ViewModelEventsCollector) -> R) -> R {
        let emission = ViewEventsEmission::begin(&self.event_dispatcher);
        callback(&emission.events_collector)
    }

    /// `_withViewEventsCollector`.
    fn with_view_events_collector<R>(&self, callback: impl FnOnce(&ViewModelEventsCollector) -> R) -> R {
        self.transactional_target.batch_changes(|| self.emit_view_event(callback))
    }

    /// `_executeCursorEdit`.
    fn execute_cursor_edit(&self, callback: impl FnOnce(&ViewModelEventsCollector)) {
        if self.cursor().context().cursor_config.read_only {
            // we cannot edit when read only...
            self.event_dispatcher
                .emit_outgoing_event(OutgoingViewModelEvent::ReadOnlyEditAttempt(ReadOnlyEditAttemptEvent::new()));
            return;
        }
        self.with_view_events_collector(callback);
    }

    /// If true, the hidden areas will be updated even if the new ranges are the same as the previous ranges
    /// (`forceUpdate`). This is because the model might have changed, which resets the hidden areas, but not the
    /// last cached value. This needs a better fix in the future.
    pub fn set_hidden_areas(&self, ranges: Vec<Range>, source: HiddenAreasSource, force_update: bool) {
        let merged_ranges = {
            let mut hidden_areas_model = self.hidden_areas_model.borrow_mut();
            hidden_areas_model.set_hidden_areas(source, ranges);
            hidden_areas_model.get_merged_ranges().to_vec()
        };
        if !force_update && self.previous_hidden_areas.borrow().as_deref() == Some(merged_ranges.as_slice()) {
            return;
        }

        *self.previous_hidden_areas.borrow_mut() = Some(merged_ranges.clone());

        let stable_viewport = self.capture_stable_viewport();

        let line_mapping_changed = self.emit_view_event(|events_collector| {
            let line_mapping_changed = self.lines.set_hidden_areas(&merged_ranges);
            if line_mapping_changed {
                self.emit_flushed_and_line_mapping_changed(events_collector);
                self.layout().on_height_maybe_changed();
            }

            let first_model_line_in_view_port =
                stable_viewport.viewport_start_model_position.map(|position| position.line_number).filter(|&line| line != 0);
            let first_model_line_is_hidden = first_model_line_in_view_port.is_some_and(|first_line| {
                merged_ranges.iter().any(|range| range.start_line_number <= first_line && first_line <= range.end_line_number)
            });
            if !first_model_line_is_hidden {
                stable_viewport.recover_viewport_start(&*self.coordinates_converter, &**self.layout());
            }
            line_mapping_changed
        });
        self.update_configuration_view_line_count.schedule(None);

        if line_mapping_changed {
            self.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::HiddenAreasChanged(HiddenAreasChangedEvent::new()));
        }
    }

    pub fn get_visible_ranges_plus_viewport_above_below(&self) -> Vec<Range> {
        let options = self.configuration.options();
        let layout_info = options.get(LAYOUT_INFO);
        let line_height = *options.get(LINE_HEIGHT);
        let lines_around = 20f64.max(js_round(layout_info.height / line_height));
        let partial_data = self.layout().get_lines_viewport_data();
        let start_view_line_number =
            1f64.max(f64::from(partial_data.completely_visible_start_line_number) - lines_around) as i32;
        let end_view_line_number = f64::from(self.get_line_count())
            .min(f64::from(partial_data.completely_visible_end_line_number) + lines_around) as i32;

        self.to_model_visible_ranges(Range::new(
            start_view_line_number,
            self.get_line_min_column(start_view_line_number),
            end_view_line_number,
            self.get_line_max_column(end_view_line_number),
        ))
    }

    pub fn get_visible_ranges(&self) -> Vec<Range> {
        let visible_view_range = self.get_completely_visible_view_range();
        self.to_model_visible_ranges(visible_view_range)
    }

    pub fn save_state(&self) -> IViewState {
        let compat_view_state = self.layout().save_state();

        let scroll_top = compat_view_state.scroll_top;
        let first_view_line_number = self.layout().get_line_number_at_vertical_offset(scroll_top);
        let first_position = self
            .coordinates_converter
            .convert_view_position_to_model_position(Position::new(first_view_line_number, self.get_line_min_column(first_view_line_number)));
        let first_position_delta_top =
            self.layout().get_vertical_offset_for_line_number(first_view_line_number, None) - scroll_top;

        IViewState {
            scroll_top: None,
            scroll_top_without_view_zones: None,
            scroll_left: compat_view_state.scroll_left,
            first_position: Some(first_position),
            first_position_delta_top,
        }
    }

    pub fn reduce_restore_state(&self, state: &IViewState) -> ReducedRestoreState {
        let Some(first_position) = state.first_position else {
            // This is a view state serialized by an older version
            return Self::reduce_restore_state_compatibility(state);
        };

        let model_position = self.model.validate_position(first_position);
        let view_position = self.coordinates_converter.convert_model_position_to_view_position(model_position, None, None, None);
        let scroll_top =
            self.layout().get_vertical_offset_for_line_number(view_position.line_number, None) - state.first_position_delta_top;
        ReducedRestoreState { scroll_left: state.scroll_left, scroll_top }
    }

    /// `_reduceRestoreStateCompatibility`: `scrollTopWithoutViewZones!` ausente é o `undefined` do original, que
    /// vira `NaN` como número.
    fn reduce_restore_state_compatibility(state: &IViewState) -> ReducedRestoreState {
        ReducedRestoreState {
            scroll_left: state.scroll_left,
            scroll_top: state.scroll_top_without_view_zones.unwrap_or(f64::NAN),
        }
    }

    // #region cursor operations

    pub fn get_selection(&self) -> Selection {
        self.cursor().get_selection()
    }

    pub fn get_selections(&self) -> Vec<Selection> {
        self.cursor().get_selections()
    }

    pub fn get_position(&self) -> Position {
        self.cursor().get_primary_cursor_state().model_state.position
    }

    pub fn set_selections(&self, source: Option<&str>, selections: &[Selection], reason: CursorChangeReason) {
        self.with_view_events_collector(|events_collector| {
            self.cursor().set_selections(events_collector, source, selections, reason);
        });
    }

    pub fn save_cursor_state(&self) -> Vec<SavedCursorState> {
        self.cursor().save_state()
    }

    pub fn restore_cursor_state(&self, states: &[SavedCursorState]) {
        self.with_view_events_collector(|events_collector| self.cursor().restore_state(events_collector, states));
    }

    pub fn execute_edits(
        &self,
        source: Option<&str>,
        edits: Vec<SingleEditOperation>,
        cursor_state_computer: &mut dyn FnMut(&[ValidEditOperation]) -> Option<Vec<Selection>>,
        reason: TextModelEditSource,
    ) {
        self.execute_cursor_edit(|events_collector| {
            self.cursor().execute_edits(events_collector, source, edits, cursor_state_computer, reason);
        });
    }

    pub fn start_composition(&self) {
        self.execute_cursor_edit(|events_collector| self.cursor().start_composition(events_collector));
    }

    pub fn end_composition(&self, source: Option<&str>) {
        self.execute_cursor_edit(|events_collector| self.cursor().end_composition(events_collector, source));
    }

    pub fn r#type(&self, text: &[u16], source: Option<&str>) {
        self.execute_cursor_edit(|events_collector| self.cursor().r#type(events_collector, text, source));
    }

    pub fn composition_type(
        &self,
        text: &[u16],
        replace_prev_char_cnt: i32,
        replace_next_char_cnt: i32,
        position_delta: i32,
        source: Option<&str>,
    ) {
        self.execute_cursor_edit(|events_collector| {
            self.cursor().composition_type(
                events_collector,
                text,
                replace_prev_char_cnt,
                replace_next_char_cnt,
                position_delta,
                source,
            );
        });
    }

    pub fn paste(
        &self,
        text: &[u16],
        paste_on_new_line: bool,
        multicursor_text: Option<&[JsString]>,
        source: Option<&str>,
        is_block: bool,
    ) {
        self.execute_cursor_edit(|events_collector| {
            self.cursor().paste(events_collector, text, paste_on_new_line, multicursor_text, source, is_block);
        });
    }

    pub fn cut(&self, source: Option<&str>) {
        self.execute_cursor_edit(|events_collector| self.cursor().cut(events_collector, source));
    }

    pub fn execute_command(&self, command: Box<dyn Command>, source: Option<&str>) {
        self.execute_cursor_edit(|events_collector| self.cursor().execute_command(events_collector, command, source));
    }

    pub fn execute_commands(&self, commands: Vec<Box<dyn Command>>, source: Option<&str>) {
        self.execute_cursor_edit(|events_collector| self.cursor().execute_commands(events_collector, commands, source));
    }

    // #endregion
}
