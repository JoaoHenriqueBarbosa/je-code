//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/inlineEditsViews/inlineEditsLineReplacementView.ts`:
//! `InlineEditsLineReplacementView`, a view de uma substituição de linhas (uma moldura sobre as linhas originais e, logo
//! abaixo, as linhas novas tokenizadas, com os trechos trocados em destaque).
//!
//! Notas de tradução:
//! - o `_edit` anônimo do construtor (`{ originalRange, modifiedRange, modifiedLines, replacements }`) é o
//!   [`LineReplacementEdit`], e o `Replacement` exportado é o [`Replacement`]; `modifiedLines: string[]` são `JsString`
//!   (colunas e recuos contam unidades UTF-16);
//! - o `n.div` da raiz (o contêiner de sobreposição com a borda em volta, a sobreposição das linhas originais, a
//!   sobreposição das linhas novas e o bloco com as linhas) é o [`LineReplacementVisual`], lido pelo pintor nativo em
//!   [`InlineEditsLineReplacementView::visual`] (ver as notas do módulo pai). O `mapOutFalsy` do original só mantém a árvore
//!   DOM estável entre recálculos; aqui há um único dado calculado, então ele não aparece;
//! - o `div` de cada linha nova (preenchido por `renderLines`) é a [`LineReplacementLine`]: a linha tokenizada mais as
//!   decorações do destaque (`inlineCompletions-modified-bubble`), que o pintor renderiza com as opções do editor
//!   (`RenderOptions.fromEditor(editor).withSetWidth(false).withScrollBeyondLastColumn(0)`), com a largura e a altura que a
//!   view calcula. O que o layout precisa da renderização, a largura mínima (`minWidthInPx`), é medido aqui por
//!   [`rendered_line_chars`], que faz o `renderOriginalLine` do original (a `RenderLineInput` com as opções do editor e o
//!   `characterMapping.getHorizontalOffset(length)`) sem montar o DOM; a largura em pixels é o máximo dessas medidas
//!   vezes `typicalHalfwidthCharacterWidth`, com `scrollBeyondLastColumn` zero;
//! - `ICodeEditor` e o `observableCodeEditor(editor)` do construtor são o `Rc<ObservableCodeEditor>` (o editor
//!   inclusive, em `editor`); `@ILanguageService` e `@IThemeService` são o `Rc<dyn ILanguageService>` (só serve ao
//!   `languageIdCodec`) e o `Rc<dyn IThemeService>`. `getModel()!` é um `expect`: sem modelo não há linhas a mostrar, e o
//!   original também lançaria;
//! - `isHovered` é o `isTargetHovered` do original. O `relativePos` do `EditorMouseEvent` é a posição do ponteiro em
//!   coordenadas do editor, que a UI nativa já entrega em `posx` e `posy` do evento. O `onclick` do nó DOM é o
//!   [`InlineEditsLineReplacementView::notify_click`] (o `onmousedown` com `preventDefault`, que impede o editor de perder o
//!   foco, é do pintor);
//! - a view zone (`domNode: $('div')`) é a [`LineReplacementViewZone`]: não tem conteúdo, só reserva a altura. As zonas são
//!   trocadas pelo `autorunDelta` do original (`autorun_delta`); o `removePreviousViewZone` ao descartar a view é o
//!   `to_disposable` do original;
//! - `intersectVertical(new OffsetRange(originalLinesOverlay.bottom, Number.MAX_SAFE_INTEGER))` é montado direto com o
//!   [`Rect`]: o `OffsetRange` do crate é inteiro e arredondaria a borda, e o limite de baixo nunca restringe;
//! - `Number.MAX_SAFE_INTEGER` da coluna final de `rangesToBubbleRanges` é `i32::MAX` (as colunas são `i32`);
//! - `asCssVariable(id)` é o próprio identificador do tema, e `getEditorBlendedColor(...).toString()` é a [`Color`] opaca
//!   (o pintor serializa).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::observable::{IObservable, IReader, Observable, ObservableExt, autorun_delta, const_observable, derived};
use je_theme::color::Color;

use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::effective_options::{EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH, FullwidthCharacterWidth};
use crate::config::editor_options::font_ligatures::{EditorFontLigatures, FONT_LIGATURES};
use crate::config::editor_options::font_size::FONT_SIZE;
use crate::config::editor_options::font_weight::FONT_WEIGHT;
use crate::config::editor_options::inline_suggest::{INLINE_SUGGEST, InlineEditsAllowCodeShifting};
use crate::config::editor_options::registry::FONT_FAMILY;
use crate::config::editor_options::scrollbar::SCROLLBAR;
use crate::config::editor_options::{EditorOption, Opt};
use crate::config::font_info::FontInfo;
use crate::core_2d::rect::Rect;
use crate::cursor_common::CursorSimpleModel;
use crate::editor_browser::{
    IViewZone, IViewZoneChangeAccessor, MouseEvent, OverlayWidgetPlacement, OverlayWidgetPosition, OverlayWidgetPositionCoordinates,
};
use crate::inline_completions::provide_inline_completions::InlineCompletionEditorType;
use crate::inline_decorations::{InlineDecoration, InlineDecorationType};
use crate::language_service::ILanguageService;
use crate::languages::ILanguageIdCodec;
use crate::line_decorations::LineDecoration;
use crate::line_range::LineRange;
use crate::line_tokens::{IViewLineTokens, LineTokens, TokenArray};
use crate::observable_code_editor::{ObservableCodeEditor, ValueOrObservable};
use crate::range::Range;
use crate::services::theme_service::IThemeService;
use crate::text_model::TextModel;
use crate::view_line_renderer::{RenderLineInput, RenderLineInputOptions, render_view_line2};
use crate::view_model_types::ViewLineRenderingData;

use super::super::inline_edits_view_interface::{IInlineEditsView, InlineEditClickEvent, InlineEditTabAction};
use super::super::theme::{
    INLINE_EDITS_BORDER_RADIUS, MODIFIED_CHANGED_LINE_BACKGROUND_COLOR, ORIGINAL_BACKGROUND_COLOR, get_editor_background_color, get_editor_blended_color,
    get_modified_border_color, get_original_border_color,
};
use super::super::utils::utils::{PrefixTrim, get_editor_valid_overlay_rect, get_prefix_trim};
use super::NonOverflowOverlayWidget;

/// `scrollbarShadow` (`scrollbar.shadow`).
const SCROLLBAR_SHADOW: &str = "scrollbar.shadow";

const DISABLE_MONOSPACE_OPTIMIZATIONS: Opt<bool> = Opt::new(EditorOption::DisableMonospaceOptimizations);
const STOP_RENDERING_LINE_AFTER: Opt<f64> = Opt::new(EditorOption::StopRenderingLineAfter);
const RENDER_WHITESPACE: Opt<String> = Opt::new(EditorOption::RenderWhitespace);
const RENDER_CONTROL_CHARACTERS: Opt<bool> = Opt::new(EditorOption::RenderControlCharacters);

/// `Replacement`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Replacement {
    pub original_range: Range,
    pub modified_range: Range,
}

/// O `_edit` do construtor: `{ originalRange, modifiedRange, modifiedLines, replacements }`.
#[derive(Clone, Debug, PartialEq)]
pub struct LineReplacementEdit {
    pub original_range: LineRange,
    pub modified_range: LineRange,
    pub modified_lines: Vec<JsString>,
    pub replacements: Vec<Replacement>,
}

/// Uma linha nova: o conteúdo tokenizado e as decorações do destaque.
pub struct ModifiedLine {
    pub tokens: LineTokens,
    pub decorations: Vec<InlineDecoration>,
}

/// O que o `_modifiedLineElements` devolve: `{ lines, requiredWidth }`.
pub struct ModifiedLineElements {
    pub lines: Vec<Rc<ModifiedLine>>,
    pub required_width: f64,
}

/// O `_layout` do original.
#[derive(Clone, Debug, PartialEq)]
pub struct LineReplacementLayout {
    pub original_lines_overlay: Rect,
    pub modified_lines_overlay: Rect,
    pub background: Rect,
    pub lower_background: Rect,
    pub lower_text: Rect,
    pub modified_line_heights: Vec<f64>,
    pub min_content_width_required: f64,
}

/// O `_viewZoneInfo` do original: `{ height, lineNumber }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineReplacementViewZoneInfo {
    pub height: f64,
    pub line_number: i32,
}

/// Uma caixa da moldura: o `div` com `position: absolute` no retângulo (em coordenadas do contêiner, já sem o
/// `contentLeft`) e `box-sizing: border-box`.
#[derive(Clone)]
pub struct LineReplacementBox {
    pub rect: Rect,
    /// O `border-radius` dos quatro cantos; nas caixas de baixo só os cantos de baixo são arredondados
    /// (`0 0 r r`), o que é [`LineReplacementBox::round_top`] falso.
    pub border_radius: f64,
    pub round_top: bool,
    /// A largura da borda em pixels.
    pub border_width: f64,
    /// A cor da borda, como identificador do tema.
    pub border_theme_color: Option<&'static str>,
    /// A cor da borda já calculada (opaca sobre o fundo do editor).
    pub border_color: Option<Rc<Color>>,
    /// A cor do fundo, como identificador do tema.
    pub background_color: Option<&'static str>,
}

/// O `boxShadow: ${scrollbarShadow} 0 6px 6px -6px`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineReplacementBoxShadow {
    /// A cor, como identificador do tema.
    pub color: &'static str,
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur: f64,
    pub spread: f64,
}

/// Uma linha nova no bloco `modifiedLinesLineReplacement`: `position: relative` com a largura e a altura calculadas.
#[derive(Clone)]
pub struct LineReplacementLine {
    pub content: Rc<ModifiedLine>,
    pub width: f64,
    pub height: f64,
}

/// O que o pintor nativo lê da view. O contêiner de sobreposição tem `overflow: hidden` e `pointer-events: none`.
#[derive(Clone)]
pub struct LineReplacementVisual {
    pub overlay_rect: Rect,
    /// `borderAroundLineReplacement`.
    pub border_around: LineReplacementBox,
    /// `originalOverlayLineReplacement`.
    pub original_overlay: LineReplacementBox,
    /// `modifiedOverlayLineReplacement` (`cursor: pointer`, `pointer-events: auto`, `overflow: hidden`).
    pub modified_overlay: LineReplacementBox,
    pub modified_overlay_shadow: LineReplacementBoxShadow,
    /// O `div` de dentro de `modifiedOverlayLineReplacement`, que cobre a caixa toda com este fundo.
    pub modified_overlay_inner_background_color: &'static str,
    /// `modifiedLinesLineReplacement` (`pointer-events: none`, `white-space: nowrap`, `overflow: hidden`).
    pub modified_lines_rect: Rect,
    pub modified_lines_border_radius: f64,
    pub font_family: Rc<String>,
    pub font_size: f64,
    pub font_weight: Rc<String>,
    pub lines: Vec<LineReplacementLine>,
}

/// A view zone do original (`domNode: $('div')`): só reserva a altura.
struct LineReplacementViewZone {
    after_line_number: i32,
    height_in_px: f64,
}

impl IViewZone for LineReplacementViewZone {
    fn after_line_number(&self) -> i32 {
        self.after_line_number
    }

    fn height_in_px(&self) -> Option<f64> {
        Some(self.height_in_px)
    }

    fn dom_node(&self) -> Rc<dyn Any> {
        Rc::new(())
    }
}

/// O `_previousViewZoneInfo`: `{ height, lineNumber, id }`.
struct PreviousViewZone {
    height: f64,
    line_number: i32,
    id: String,
}

/// `renderOriginalLine` de `renderLines.ts` sem o DOM: o `maxCharWidth` da linha, o deslocamento horizontal do último
/// caractere da `RenderLineInput` montada com as opções do editor, em unidades de `typicalHalfwidthCharacterWidth`.
pub(super) fn rendered_line_chars(editor: &Rc<CodeEditorWidget>, tokens: &LineTokens, decorations: &[InlineDecoration]) -> f64 {
    let font_info = editor.get_option(FONT_INFO);
    let line_content = tokens.get_line_content_ref().clone();
    let is_basic_ascii = ViewLineRenderingData::is_basic_ascii(line_content.units(), true);
    let contains_rtl = ViewLineRenderingData::contains_rtl(line_content.units(), is_basic_ascii, true);
    let tab_size = editor.get_model().map_or(0, |model| model.get_options().tab_size as i32);
    let line_tokens: Rc<dyn IViewLineTokens> = Rc::new(tokens.clone());

    let output = render_view_line2(&RenderLineInput::new(RenderLineInputOptions {
        use_monospace_optimizations: font_info.is_monospace && !*editor.get_option(DISABLE_MONOSPACE_OPTIMIZATIONS),
        can_use_halfwidth_rightwards_arrow: font_info.can_use_halfwidth_rightwards_arrow,
        line_content,
        continues_with_wrapped_line: false,
        is_basic_ascii,
        contains_rtl,
        faux_indent_length: 0,
        line_tokens,
        line_decorations: LineDecoration::filter(decorations, 1, 1, i32::MAX),
        tab_size,
        start_visible_column: 0,
        space_width: font_info.space_width,
        middot_width: font_info.middot_width,
        wsmiddot_width: font_info.wsmiddot_width,
        stop_rendering_line_after: *editor.get_option(STOP_RENDERING_LINE_AFTER) as i32,
        render_whitespace: editor.get_option(RENDER_WHITESPACE).clone(),
        render_control_characters: *editor.get_option(RENDER_CONTROL_CHARACTERS),
        font_ligatures: editor.get_option(FONT_LIGATURES).as_str() != EditorFontLigatures::OFF,
        // Send no selections, original line cannot be selected
        selections_on_line: None,
        text_direction: None,
        vertical_scrollbar_size: editor.get_option(SCROLLBAR).vertical_scrollbar_size,
        render_new_line_when_empty: false,
        use_two_cell_fullwidth_characters: *editor.get_option(EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH) == FullwidthCharacterWidth::TwoCells,
    }));
    f64::from(output.character_mapping.get_horizontal_offset(output.character_mapping.length as i32))
}

/// `textModel.tokenization.tokenizeLinesAt(lineNumber, [line])?.[0]` convertido em `LineTokens` da linha (vazios quando o
/// modelo não tokeniza). A `InlineEditsInsertionView` mede as linhas do mesmo modo.
pub(super) fn tokenize_line_for_render(
    text_model: &TextModel,
    line_number: i32,
    line: &JsString,
    language_id_codec: &Rc<dyn ILanguageIdCodec>,
) -> LineTokens {
    let tokenized = text_model.tokenization().tokenize_lines_at(line_number, std::slice::from_ref(line)).and_then(|lines| lines.into_iter().next());
    match tokenized {
        Some(t) => TokenArray::from_line_tokens(&t).to_line_tokens(line, Rc::clone(language_id_codec)),
        None => LineTokens::create_empty(line.clone(), Rc::clone(language_id_codec)),
    }
}

/// `rangesToBubbleRanges(ranges)`: as faixas de várias linhas viram uma faixa por linha.
fn ranges_to_bubble_ranges(mut ranges: Vec<Range>) -> Vec<Range> {
    let mut result: Vec<Range> = Vec::new();
    while !ranges.is_empty() {
        let mut range = ranges.remove(0);
        if range.start_line_number != range.end_line_number {
            ranges.push(Range::new(range.start_line_number + 1, 1, range.end_line_number, range.end_column));
            range = Range::new(range.start_line_number, range.start_column, range.start_line_number, i32::MAX); // o original também não é exato aqui
        }

        result.push(range);
    }
    result
}

/// O `_modifiedLineElements` do original.
fn compute_modified_line_elements(
    editor: &Rc<ObservableCodeEditor>,
    language_service: &Rc<dyn ILanguageService>,
    font_info: &IObservable<Rc<FontInfo>>,
    prefix_trim: &PrefixTrim,
    edit: &LineReplacementEdit,
    reader: &dyn IReader,
) -> ModifiedLineElements {
    let mut lines: Vec<Rc<ModifiedLine>> = Vec::new();
    let mut required_width = 0.0_f64;

    let max_prefix_trim = prefix_trim.prefix_trim;
    let modified_bubbles: Vec<Range> = ranges_to_bubble_ranges(edit.replacements.iter().map(|r| r.modified_range).collect())
        .into_iter()
        .map(|r| Range::new(r.start_line_number, r.start_column - max_prefix_trim, r.end_line_number, r.end_column - max_prefix_trim))
        .collect();

    let text_model = editor.model.get().expect("as linhas novas só são mostradas com modelo");
    let language_id_codec = language_service.language_id_codec();
    let start_line_number = edit.modified_range.start_line_number;
    for i in 0..edit.modified_range.length() {
        let line_number = start_line_number + i;
        let original_mod_line = &edit.modified_lines[i as usize];
        let trim = usize::try_from(max_prefix_trim).unwrap_or(0).min(original_mod_line.len());
        let mod_line = original_mod_line.slice(trim, original_mod_line.len());

        let tokens = tokenize_line_for_render(&text_model, line_number, &mod_line, &language_id_codec);

        let mut decorations: Vec<InlineDecoration> = Vec::new();
        for modified in modified_bubbles.iter().filter(|b| b.start_line_number == line_number) {
            let validated_end_column = modified.end_column.min(mod_line.len() as i32 + 1);
            decorations.push(InlineDecoration::new(
                Range::new(1, modified.start_column, 1, validated_end_column),
                "inlineCompletions-modified-bubble",
                InlineDecorationType::Regular,
            ));
        }

        // todas as linhas poderiam ser renderizadas de uma vez só, num único elemento
        let max_chars = rendered_line_chars(&editor.editor, &tokens, &decorations);
        // update when font info changes
        let typical_halfwidth_character_width = font_info.read(reader).typical_halfwidth_character_width;

        required_width = required_width.max(max_chars * typical_halfwidth_character_width);

        lines.push(Rc::new(ModifiedLine { tokens, decorations }));
    }

    ModifiedLineElements { lines, required_width }
}

/// `InlineEditsLineReplacementView`.
pub struct InlineEditsLineReplacementView {
    base: Rc<Disposable>,
    on_did_click: Rc<Emitter<InlineEditClickEvent>>,
    is_hovered: IObservable<bool>,
    min_editor_scroll_height: IObservable<f64>,
    visual: IObservable<Option<Rc<LineReplacementVisual>>>,
}

impl InlineEditsLineReplacementView {
    pub fn new(
        editor: Rc<ObservableCodeEditor>,
        edit: IObservable<Option<Rc<LineReplacementEdit>>>,
        editor_type: IObservable<InlineCompletionEditorType>,
        tab_action: IObservable<InlineEditTabAction>,
        language_service: Rc<dyn ILanguageService>,
        theme_service: Rc<dyn IThemeService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_click = Emitter::<InlineEditClickEvent>::new();
        base.register(on_did_click.clone());

        let max_prefix_trim: IObservable<Option<Rc<PrefixTrim>>> = {
            let editor = Rc::clone(&editor);
            edit.map(move |e, reader| {
                e.map(|e| {
                    let ranges: Vec<Range> = e.replacements.iter().flat_map(|r| [r.original_range, r.modified_range]).collect();
                    Rc::new(get_prefix_trim(&ranges, e.original_range, &e.modified_lines, &editor.editor, Some(reader)))
                })
            })
        };

        let modified_line_elements: IObservable<Option<Rc<ModifiedLineElements>>> = {
            let (editor, edit, max_prefix_trim, language_service) =
                (Rc::clone(&editor), Rc::clone(&edit), Rc::clone(&max_prefix_trim), Rc::clone(&language_service));
            let font_info = editor.get_option(FONT_INFO);
            derived(move |reader| {
                let prefix_trim = max_prefix_trim.read(reader);
                let edit = edit.read(reader);
                let (Some(prefix_trim), Some(edit)) = (prefix_trim, edit) else {
                    return None;
                };
                Some(Rc::new(compute_modified_line_elements(&editor, &language_service, &font_info, &prefix_trim, &edit, reader)))
            })
        };

        let layout: IObservable<Option<Rc<LineReplacementLayout>>> = {
            let (editor, edit, max_prefix_trim, modified_line_elements) =
                (Rc::clone(&editor), Rc::clone(&edit), Rc::clone(&max_prefix_trim), Rc::clone(&modified_line_elements));
            derived(move |reader| {
                let modified_lines = modified_line_elements.read(reader);
                let max_prefix_trim = max_prefix_trim.read(reader);
                let edit = edit.read(reader);
                let (Some(modified_lines), Some(max_prefix_trim), Some(edit)) = (modified_lines, max_prefix_trim, edit) else {
                    return None;
                };

                let prefix_left_offset = max_prefix_trim.prefix_left_offset;
                let required_width = modified_lines.required_width;

                let original_line_heights = editor.observe_line_heights_for_line_range(ValueOrObservable::Value(edit.original_range)).read(reader);
                let modified_line_heights: Vec<f64> = {
                    let modified_length = edit.modified_range.length() as usize;
                    let mut line_heights: Vec<f64> = original_line_heights.iter().take(modified_length).copied().collect();
                    while line_heights.len() < modified_length {
                        // `originalLineHeights[length - 1]` sem linhas é `undefined`, que soma como `NaN`.
                        line_heights.push(original_line_heights.last().copied().unwrap_or(f64::NAN));
                    }
                    line_heights
                };

                let content_left = editor.layout_info_content_left.read(reader);
                let vertical_scrollbar_width = editor.layout_info_vertical_scrollbar_width.read(reader);
                let scroll_left = editor.scroll_left.read(reader);
                let scroll_top = editor.scroll_top.read(reader);
                let editor_left_offset = content_left - scroll_left;

                let text_model = editor.editor.get_model().expect("as linhas novas só são mostradas com modelo");

                let original_line_widths = edit
                    .original_range
                    .map_to_line_array(|line| editor.editor.get_offset_for_column(line, text_model.get_line_max_column(line)) - prefix_left_offset);
                let max_line_width = original_line_widths.iter().copied().fold(required_width, f64::max);

                let start_line_number = edit.original_range.start_line_number;
                let end_line_number = edit.original_range.end_line_number_exclusive - 1;
                let top_of_original_lines = editor.editor.get_top_for_line_number(start_line_number, None) - scroll_top;
                let bottom_of_original_lines = editor.editor.get_bottom_for_line_number(end_line_number, None) - scroll_top;

                // Box Widget positioning
                let original_lines_overlay = Rect::from_left_top_width_height(
                    editor_left_offset + prefix_left_offset,
                    top_of_original_lines,
                    max_line_width,
                    bottom_of_original_lines - top_of_original_lines,
                );
                let modified_lines_overlay = Rect::from_left_top_width_height(
                    original_lines_overlay.left,
                    original_lines_overlay.bottom,
                    original_lines_overlay.width(),
                    modified_line_heights.iter().sum::<f64>(),
                );
                let background = Rect::hull(&[original_lines_overlay, modified_lines_overlay]);

                let lower_top = background.top.max(original_lines_overlay.bottom);
                let lower_background = Rect::new(background.left, lower_top, background.right, lower_top.max(background.bottom));
                let lower_text = Rect::new(lower_background.left, lower_background.top, lower_background.right, lower_background.bottom);

                Some(Rc::new(LineReplacementLayout {
                    original_lines_overlay,
                    modified_lines_overlay,
                    background,
                    lower_background,
                    lower_text,
                    modified_line_heights,
                    min_content_width_required: prefix_left_offset + max_line_width + vertical_scrollbar_width,
                }))
            })
        };

        let view_zone_info: IObservable<Option<Rc<LineReplacementViewZoneInfo>>> = {
            let (layout, edit) = (Rc::clone(&layout), Rc::clone(&edit));
            let allow_code_shifting_always = editor.get_option(INLINE_SUGGEST).map(|o, _| o.edits.allow_code_shifting == InlineEditsAllowCodeShifting::Always);
            derived(move |reader| {
                if !allow_code_shifting_always.read(reader) {
                    return None;
                }

                let layout = layout.read(reader);
                let edit = edit.read(reader);
                let (Some(layout), Some(edit)) = (layout, edit) else {
                    return None;
                };

                let view_zone_height = layout.lower_background.height();
                let view_zone_line_number = edit.original_range.end_line_number_exclusive;
                Some(Rc::new(LineReplacementViewZoneInfo { height: view_zone_height, line_number: view_zone_line_number }))
            })
        };

        let min_editor_scroll_height: IObservable<f64> = {
            let (layout, view_zone_info, editor) = (Rc::clone(&layout), Rc::clone(&view_zone_info), Rc::clone(&editor));
            derived(move |reader| {
                let layout = layout.read(reader);
                let view_zone_info = view_zone_info.read(reader);
                match layout {
                    Some(layout) if view_zone_info.is_none() => layout.lower_text.bottom + editor.editor.get_scroll_top(),
                    _ => 0.0,
                }
            })
        };

        // `_div`
        let visual: IObservable<Option<Rc<LineReplacementVisual>>> = {
            let (editor, layout, modified_line_elements, editor_type, theme_service) =
                (Rc::clone(&editor), Rc::clone(&layout), Rc::clone(&modified_line_elements), Rc::clone(&editor_type), Rc::clone(&theme_service));
            let overlay_rect = get_editor_valid_overlay_rect(&editor);
            let original_border_color = get_original_border_color(&tab_action);
            let modified_border_color = get_modified_border_color(&tab_action);
            let (font_family, font_size, font_weight) = (editor.get_option(FONT_FAMILY), editor.get_option(FONT_SIZE), editor.get_option(FONT_WEIGHT));
            derived(move |reader| {
                let layout_props = layout.read(reader)?;
                let modified_line_elements = modified_line_elements.read(reader)?;

                let content_left = editor.layout_info_content_left.read(reader);

                let editor_type = editor_type.read(reader);
                let separator_width = if editor_type == InlineCompletionEditorType::DiffEditor { 3.0 } else { 1.0 };

                let modified_border_color = modified_border_color.read(reader);
                let original_border_color = original_border_color.read(reader);
                let editor_background = get_editor_background_color(editor_type);

                let lines: Vec<LineReplacementLine> = modified_line_elements
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(i, content)| LineReplacementLine {
                        content: Rc::clone(content),
                        width: layout_props.lower_text.width(),
                        height: layout_props.modified_line_heights[i],
                    })
                    .collect();

                Some(Rc::new(LineReplacementVisual {
                    overlay_rect: overlay_rect.read(reader),
                    border_around: LineReplacementBox {
                        rect: layout_props.background.translate_x(-content_left).with_margin(separator_width),
                        border_radius: INLINE_EDITS_BORDER_RADIUS,
                        round_top: true,
                        border_width: separator_width + 1.0,
                        border_theme_color: Some(editor_background),
                        border_color: None,
                        background_color: None,
                    },
                    original_overlay: LineReplacementBox {
                        rect: layout_props.background.translate_x(-content_left),
                        border_radius: INLINE_EDITS_BORDER_RADIUS,
                        round_top: true,
                        border_width: 1.0,
                        border_theme_color: None,
                        border_color: Some(get_editor_blended_color(ValueOrObservable::Value(original_border_color), &theme_service).read(reader)),
                        background_color: Some(ORIGINAL_BACKGROUND_COLOR),
                    },
                    modified_overlay: LineReplacementBox {
                        rect: layout_props.lower_background.translate_x(-content_left),
                        border_radius: INLINE_EDITS_BORDER_RADIUS,
                        round_top: false,
                        border_width: 1.0,
                        border_theme_color: Some(modified_border_color),
                        border_color: None,
                        background_color: Some(editor_background),
                    },
                    modified_overlay_shadow: LineReplacementBoxShadow { color: SCROLLBAR_SHADOW, offset_x: 0.0, offset_y: 6.0, blur: 6.0, spread: -6.0 },
                    modified_overlay_inner_background_color: MODIFIED_CHANGED_LINE_BACKGROUND_COLOR,
                    modified_lines_rect: layout_props.lower_text.translate_x(-content_left),
                    modified_lines_border_radius: INLINE_EDITS_BORDER_RADIUS,
                    font_family: font_family.read(reader),
                    font_size: *font_size.read(reader),
                    font_weight: font_weight.read(reader),
                    lines,
                }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        let is_hovered: IObservable<bool> = {
            let layout = Rc::clone(&layout);
            editor.is_target_hovered(
                move |e| {
                    let Some(layout) = layout.get() else {
                        return false;
                    };
                    layout.lower_background.contains_point(crate::core_2d::point::Point::new(e.event.posx, e.event.posy))
                },
                base.store(),
            )
        };

        // View Zones
        let previous_view_zone: Rc<RefCell<Option<PreviousViewZone>>> = Rc::new(RefCell::new(None));

        base.register(to_disposable({
            let (editor, previous_view_zone) = (Rc::clone(&editor), Rc::clone(&previous_view_zone));
            move || {
                let observable_editor = Rc::clone(&editor);
                editor.editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| remove_previous_view_zone(&observable_editor, &previous_view_zone, accessor));
            }
        }));

        base.register(autorun_delta(Rc::clone(&view_zone_info), {
            let (editor, previous_view_zone) = (Rc::clone(&editor), Rc::clone(&previous_view_zone));
            move |delta| {
                let last_value = delta.last_value.flatten();
                let new_value = delta.new_value;
                let key = |info: &Option<Rc<LineReplacementViewZoneInfo>>| info.as_ref().map(|i| (i.height, i.line_number));
                if key(&last_value) == key(&new_value) {
                    return;
                }
                let observable_editor = Rc::clone(&editor);
                editor.editor.change_view_zones(&mut |accessor: &mut dyn IViewZoneChangeAccessor| {
                    remove_previous_view_zone(&observable_editor, &previous_view_zone, accessor);
                    let Some(new_value) = &new_value else {
                        return;
                    };
                    add_view_zone(&observable_editor, &previous_view_zone, new_value, accessor);
                });
            }
        }));

        base.register(editor.create_overlay_widget(NonOverflowOverlayWidget::new(
            const_observable(Some(OverlayWidgetPosition {
                preference: Some(OverlayWidgetPlacement::Coordinates(OverlayWidgetPositionCoordinates { top: 0.0, left: 0.0 })),
                stack_ordinal: None,
            })),
            derived(move |reader| layout.read(reader).map_or(0.0, |layout| layout.min_content_width_required)),
        )));

        Rc::new(Self { base, on_did_click, is_hovered, min_editor_scroll_height, visual })
    }

    /// O dado que o pintor nativo lê (`None` sem layout).
    pub fn visual(&self) -> IObservable<Option<Rc<LineReplacementVisual>>> {
        Rc::clone(&self.visual)
    }

    /// O `onclick` do `modifiedOverlayLineReplacement`: o pintor entrega o evento.
    pub fn notify_click(&self, event: MouseEvent) {
        self.on_did_click.fire(InlineEditClickEvent::new(event, false));
    }
}

/// `removePreviousViewZone(changeAccessor)`.
fn remove_previous_view_zone(
    editor: &Rc<ObservableCodeEditor>,
    previous_view_zone: &Rc<RefCell<Option<PreviousViewZone>>>,
    change_accessor: &mut dyn IViewZoneChangeAccessor,
) {
    let Some(previous) = previous_view_zone.borrow_mut().take() else {
        return;
    };

    change_accessor.remove_zone(&previous.id);

    let cursor_line_number = editor.cursor_line_number.get();
    if cursor_line_number.is_some_and(|line| line >= previous.line_number) {
        editor.editor.set_scroll_top(editor.scroll_top.get() - previous.height, None);
    }
}

/// `addViewZone(viewZoneInfo, changeAccessor)`.
fn add_view_zone(
    editor: &Rc<ObservableCodeEditor>,
    previous_view_zone: &Rc<RefCell<Option<PreviousViewZone>>>,
    view_zone_info: &LineReplacementViewZoneInfo,
    change_accessor: &mut dyn IViewZoneChangeAccessor,
) {
    let active_view_zone = change_accessor.add_zone(Rc::new(LineReplacementViewZone {
        after_line_number: view_zone_info.line_number - 1,
        height_in_px: view_zone_info.height, // move computation to layout?
    }));

    *previous_view_zone.borrow_mut() =
        Some(PreviousViewZone { height: view_zone_info.height, line_number: view_zone_info.line_number, id: active_view_zone });

    let cursor_line_number = editor.cursor_line_number.get();
    if cursor_line_number.is_some_and(|line| line >= view_zone_info.line_number) {
        editor.editor.set_scroll_top(editor.scroll_top.get() + view_zone_info.height, None);
    }
}

impl IInlineEditsView for InlineEditsLineReplacementView {
    fn is_hovered(&self) -> IObservable<bool> {
        Rc::clone(&self.is_hovered)
    }

    fn min_editor_scroll_height(&self) -> Option<IObservable<f64>> {
        Some(Rc::clone(&self.min_editor_scroll_height))
    }

    fn on_did_click(&self) -> Event<InlineEditClickEvent> {
        self.on_did_click.event()
    }
}

impl IDisposable for InlineEditsLineReplacementView {
    fn dispose(&self) {
        self.base.dispose();
    }
}
