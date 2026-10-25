//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/renderLines.ts`: a renderização das linhas apagadas
//! (ou antigas) que o editor de diff põe numa zona de visão do editor modificado, sobre o `renderViewLine2` do porte
//! ([`crate::view_line_renderer`]).
//!
//! Divergências:
//! - **DOM vira dado.** O `renderLines` do original escreve o HTML das linhas no `innerHTML` do `domNode` e aplica a fonte nele
//!   (`applyFontInfo`). Aqui não há `domNode`: a função devolve o [`RenderLinesResult`], que carrega o [`FontInfo`] a aplicar
//!   ([`RenderLinesResult::font_info`]) e uma [`RenderedViewLine`] por linha de visão (a posição `top`, as classes `view-line` e
//!   `char-delete`, a largura e o HTML do `renderViewLine2`, que é a especificação das partes que o pintor desenha, como diz
//!   [`crate::view_line_renderer`]). [`RenderedViewLine::to_html`] e [`RenderLinesResult::html`] reproduzem byte a byte o HTML que o
//!   original escreve, para quem precisar dele como especificação. O `TrustedTypes` (`ttPolicy`) só existe no navegador e não é
//!   portado.
//! - **`getModelPositionAt(domNode, offset)`.** O original sobe da `span` até a `div.view-line`, acha o índice da linha entre as
//!   irmãs (`querySelectorAll('.view-line')`) e conta as irmãs anteriores da `span` (o `getColumnOfNodeOffset` de `viewLine.ts`, que
//!   entra aqui como a chamada direta de [`CharacterMapping::get_column`], porque uma função de repasse não existe neste projeto).
//!   O pintor já sabe a que linha, parte e deslocamento um ponto da tela pertence: o ponto chega como [`ViewLineDomPoint`], e `None`
//!   (a `span` fora de uma `view-line`) é o `undefined` do original.
//! - **`LineSource`.** O `lineBreakData` e as duas flags têm os padrões do original (`null` por linha, `true`, `true`) em
//!   [`LineSource::from_tokens`]; [`LineSource::new`] recebe todos os argumentos. A fonte é movida para o resultado, que a consulta no
//!   `getModelPositionAt`.
//! - **`RenderOptions`.** O construtor posicional é o literal da estrutura (todos os campos são públicos); os padrões
//!   `useTwoCellFullwidthCharacters = false` e `setWidth = true` ficam com quem constrói. `renderWhitespace` e `fontLigatures` são as
//!   `String` das opções, e `stopRenderingLineAfter` fica `f64`, como o número do original.
//! - **`Number.MAX_SAFE_INTEGER`** do `LineDecoration.filter` é `MAX_SAFE_SMALL_INTEGER`: o valor só é o fim de uma decoração que
//!   atravessa a linha, e o `extractWrapped` o recorta ao tamanho da linha.

use std::rc::Rc;

use je_base::JsString;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;

use crate::code_editor_widget::ICodeEditor;
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::effective_options::{EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH, FullwidthCharacterWidth};
use crate::config::editor_options::font_ligatures::{EditorFontLigatures, FONT_LIGATURES};
use crate::config::editor_options::layout_info::LAYOUT_INFO;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::scrollbar::SCROLLBAR;
use crate::config::editor_options::{EditorOption, Opt};
use crate::config::font_info::FontInfo;
use crate::inline_completions::view::inline_edits::inline_edits_views::inline_edits_line_replacement_view::{
    DISABLE_MONOSPACE_OPTIMIZATIONS, RENDER_CONTROL_CHARACTERS, RENDER_WHITESPACE, STOP_RENDERING_LINE_AFTER,
};
use crate::inline_decorations::InlineDecoration;
use crate::line_decorations::LineDecoration;
use crate::line_tokens::{IViewLineTokens, LineTokens};
use crate::model_line_projection_data::ModelLineProjectionData;
use crate::position::Position;
use crate::view_line_renderer::{DomPosition, RenderLineInput, RenderLineInputOptions, RenderLineOutput, js_number_to_string, render_view_line2};
use crate::view_model_types::ViewLineRenderingData;

/// `EditorOption.scrollBeyondLastColumn`.
const SCROLL_BEYOND_LAST_COLUMN: Opt<f64> = Opt::new(EditorOption::ScrollBeyondLastColumn);

/// `renderLines(source, options, decorations, domNode, noExtra)`, sem o `domNode` (ver as divergências do módulo).
pub fn render_lines(source: LineSource, options: &RenderOptions, decorations: &[InlineDecoration], no_extra: bool) -> RenderLinesResult {
    let has_char_changes = !decorations.is_empty();

    let mut max_chars_per_line = 0.0_f64;
    let mut rendered_line_count = 0_usize;
    let mut view_line_counts: Vec<usize> = Vec::new();
    let mut lines: Vec<RenderedViewLine> = Vec::new();
    for line_index in 0..source.line_tokens.len() {
        let line_number = line_index as i32 + 1;
        let line_tokens = &source.line_tokens[line_index];
        let actual_decorations = LineDecoration::filter(decorations, line_number, 1, MAX_SAFE_SMALL_INTEGER);

        if let Some(line_break_data) = source.line_break_data.get(line_index).and_then(Option::as_ref) {
            let mut last_break_offset = 0_usize;
            for &break_offset in &line_break_data.break_offsets {
                let view_line_tokens: Rc<dyn IViewLineTokens> = Rc::new(line_tokens.slice_and_inflate(last_break_offset as i32, break_offset as i32, 0));
                let (line, max_char_width) = render_original_line(
                    rendered_line_count,
                    view_line_tokens,
                    LineDecoration::extract_wrapped(&actual_decorations, last_break_offset as i32, break_offset as i32),
                    has_char_changes,
                    &source,
                    options,
                    no_extra,
                    last_break_offset,
                );
                max_chars_per_line = max_chars_per_line.max(max_char_width);
                lines.push(line);
                rendered_line_count += 1;
                last_break_offset = break_offset;
            }
            view_line_counts.push(line_break_data.break_offsets.len());
        } else {
            view_line_counts.push(1);
            let (line, max_char_width) = render_original_line(
                rendered_line_count,
                Rc::new(line_tokens.clone()),
                actual_decorations,
                has_char_changes,
                &source,
                options,
                no_extra,
                0,
            );
            max_chars_per_line = max_chars_per_line.max(max_char_width);
            lines.push(line);
            rendered_line_count += 1;
        }
    }
    max_chars_per_line += options.scroll_beyond_last_column;

    let min_width_in_px = max_chars_per_line * options.typical_halfwidth_character_width;

    RenderLinesResult { height_in_lines: rendered_line_count, min_width_in_px, view_line_counts, font_info: options.font_info.clone(), lines, source }
}

/// `LineSource`: as linhas a renderizar, os dados de quebra de cada uma e as flags que dispensam a checagem de ASCII básico e de RTL.
pub struct LineSource {
    pub line_tokens: Vec<LineTokens>,
    pub line_break_data: Vec<Option<ModelLineProjectionData>>,
    pub might_contain_non_basic_ascii: bool,
    pub might_contain_rtl: bool,
}

impl LineSource {
    /// `new LineSource(lineTokens, lineBreakData, mightContainNonBasicASCII, mightContainRTL)`.
    pub fn new(
        line_tokens: Vec<LineTokens>,
        line_break_data: Vec<Option<ModelLineProjectionData>>,
        might_contain_non_basic_ascii: bool,
        might_contain_rtl: bool,
    ) -> Self {
        Self { line_tokens, line_break_data, might_contain_non_basic_ascii, might_contain_rtl }
    }

    /// `new LineSource(lineTokens)`: sem quebra de linha e com as duas flags em `true`, os padrões do original.
    pub fn from_tokens(line_tokens: Vec<LineTokens>) -> Self {
        let line_break_data = line_tokens.iter().map(|_| None).collect();
        Self::new(line_tokens, line_break_data, true, true)
    }
}

/// `RenderOptions`.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub tab_size: i32,
    pub font_info: FontInfo,
    pub disable_monospace_optimizations: bool,
    pub typical_halfwidth_character_width: f64,
    pub scroll_beyond_last_column: f64,
    pub line_height: f64,
    pub line_decorations_width: f64,
    pub stop_rendering_line_after: f64,
    /// `'none' | 'boundary' | 'selection' | 'trailing' | 'all'`.
    pub render_whitespace: String,
    pub render_control_characters: bool,
    pub font_ligatures: String,
    pub vertical_scrollbar_size: f64,
    pub use_two_cell_fullwidth_characters: bool,
    pub set_width: bool,
}

impl RenderOptions {
    /// `RenderOptions.fromEditor(editor)`.
    pub fn from_editor<E: ICodeEditor + ?Sized>(editor: &E) -> RenderOptions {
        let modified_editor_options = editor.get_options();
        let font_info = modified_editor_options.get(FONT_INFO).clone();
        let layout_info = modified_editor_options.get(LAYOUT_INFO);

        RenderOptions {
            tab_size: editor.get_model().map_or(0, |model| model.get_options().tab_size),
            disable_monospace_optimizations: *modified_editor_options.get(DISABLE_MONOSPACE_OPTIMIZATIONS),
            typical_halfwidth_character_width: font_info.typical_halfwidth_character_width,
            scroll_beyond_last_column: *modified_editor_options.get(SCROLL_BEYOND_LAST_COLUMN),
            line_height: *modified_editor_options.get(LINE_HEIGHT),
            line_decorations_width: layout_info.decorations_width,
            stop_rendering_line_after: *modified_editor_options.get(STOP_RENDERING_LINE_AFTER),
            render_whitespace: modified_editor_options.get(RENDER_WHITESPACE).clone(),
            render_control_characters: *modified_editor_options.get(RENDER_CONTROL_CHARACTERS),
            font_ligatures: modified_editor_options.get(FONT_LIGATURES).clone(),
            vertical_scrollbar_size: modified_editor_options.get(SCROLLBAR).vertical_scrollbar_size,
            use_two_cell_fullwidth_characters: *modified_editor_options.get(EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH) == FullwidthCharacterWidth::TwoCells,
            set_width: true,
            font_info,
        }
    }

    /// `withSetWidth(setWidth)`.
    pub fn with_set_width(&self, set_width: bool) -> RenderOptions {
        RenderOptions { set_width, ..self.clone() }
    }

    /// `withScrollBeyondLastColumn(scrollBeyondLastColumn)`.
    pub fn with_scroll_beyond_last_column(&self, scroll_beyond_last_column: f64) -> RenderOptions {
        RenderOptions { scroll_beyond_last_column, ..self.clone() }
    }
}

/// O ponto da tela que o `getModelPositionAt` recebe no lugar do par `(span, offset)` do DOM: a linha de visão (o índice entre as
/// `div.view-line`), a parte (o índice da `span` entre as irmãs), o comprimento do texto da parte (`span.textContent.length`) e o
/// deslocamento dentro dela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewLineDomPoint {
    pub view_line_index: usize,
    pub part_index: u32,
    pub part_length: u32,
    pub offset: u32,
}

/// `RenderLinesResult`.
pub struct RenderLinesResult {
    pub height_in_lines: usize,
    pub min_width_in_px: f64,
    pub view_line_counts: Vec<usize>,
    /// A fonte que o original aplica ao `domNode` (`applyFontInfo`).
    pub font_info: FontInfo,
    /// Uma entrada por linha de visão renderizada.
    pub lines: Vec<RenderedViewLine>,
    source: LineSource,
}

impl RenderLinesResult {
    /// O HTML que o original escreve no `innerHTML` do `domNode`.
    pub fn html(&self) -> JsString {
        let mut html = JsString::new();
        for line in &self.lines {
            html.push_units(line.to_html().units());
        }
        html
    }

    /// `getModelPositionAt(domNode, offset)`: a posição do modelo (no texto da fonte, com a primeira linha em 1) de um ponto da tela, ou
    /// `None` quando ele não está numa linha renderizada.
    pub fn get_model_position_at(&self, point: &ViewLineDomPoint) -> Option<Position> {
        let render_output = self.lines.get(point.view_line_index)?;

        // Mapeia o índice da linha de visão de volta para a linha do modelo.
        let mut model_line_number = 1_usize;
        let mut remaining_view_lines = point.view_line_index;
        for (i, &view_line_count) in self.view_line_counts.iter().enumerate() {
            if remaining_view_lines < view_line_count {
                model_line_number = i + 1;
                break;
            }
            remaining_view_lines -= view_line_count;
        }

        if model_line_number > self.source.line_tokens.len() {
            return None;
        }

        let dom_position = DomPosition::new(point.part_index, point.offset);
        let column = render_output.output.output.character_mapping.get_column(&dom_position, point.part_length) + render_output.output.offset as i32;

        Some(Position::new(model_line_number as i32, column))
    }
}

/// Uma linha de visão renderizada: a `div.view-line` do original.
pub struct RenderedViewLine {
    /// O `top:` do estilo, em pixels (`viewLineIdx * lineHeight`).
    pub top: f64,
    /// A classe `char-delete` (a linha inteira foi apagada, sem mudanças de caractere).
    pub char_delete: bool,
    /// A largura de `1000000px` do estilo (`options.setWidth`).
    pub set_width: bool,
    /// O HTML do `renderViewLine2`: as partes dentro da `div`.
    pub html: JsString,
    pub output: RenderLineOutputWithOffset,
}

impl RenderedViewLine {
    /// A `div.view-line` inteira, como o original a escreve no `StringBuilder`.
    pub fn to_html(&self) -> JsString {
        let mut prefix = String::from("<div class=\"view-line");
        if self.char_delete {
            prefix.push_str(" char-delete");
        }
        prefix.push_str("\" style=\"top:");
        prefix.push_str(&js_number_to_string(self.top));
        prefix.push_str(if self.set_width { "px;width:1000000px;\">" } else { "px;\">" });

        let mut html = JsString::from(prefix);
        html.push_units(self.html.units());
        html.push_units(JsString::from("</div>").units());
        html
    }
}

/// `RenderLineOutputWithOffset` (`extends RenderLineOutput`): a saída de uma linha de visão e o deslocamento dela na linha do modelo.
pub struct RenderLineOutputWithOffset {
    pub output: RenderLineOutput,
    pub offset: usize,
}

/// `renderOriginalLine(...)`: devolve a linha de visão e `maxCharWidth`. O `offset` é o começo da linha de visão na linha do modelo.
#[allow(clippy::too_many_arguments)]
fn render_original_line(
    view_line_idx: usize,
    line_tokens: Rc<dyn IViewLineTokens>,
    decorations: Vec<LineDecoration>,
    has_char_changes: bool,
    source: &LineSource,
    options: &RenderOptions,
    no_extra: bool,
    offset: usize,
) -> (RenderedViewLine, f64) {
    let line_content = line_tokens.get_line_content();
    let is_basic_ascii = ViewLineRenderingData::is_basic_ascii(line_content.units(), source.might_contain_non_basic_ascii);
    let contains_rtl = ViewLineRenderingData::contains_rtl(line_content.units(), is_basic_ascii, source.might_contain_rtl);
    let output = render_view_line2(&RenderLineInput::new(RenderLineInputOptions {
        use_monospace_optimizations: options.font_info.is_monospace && !options.disable_monospace_optimizations,
        can_use_halfwidth_rightwards_arrow: options.font_info.can_use_halfwidth_rightwards_arrow,
        line_content,
        continues_with_wrapped_line: false,
        is_basic_ascii,
        contains_rtl,
        faux_indent_length: 0,
        line_tokens,
        line_decorations: decorations,
        tab_size: options.tab_size,
        start_visible_column: 0,
        space_width: options.font_info.space_width,
        middot_width: options.font_info.middot_width,
        wsmiddot_width: options.font_info.wsmiddot_width,
        stop_rendering_line_after: options.stop_rendering_line_after as i32,
        render_whitespace: options.render_whitespace.clone(),
        render_control_characters: options.render_control_characters,
        font_ligatures: options.font_ligatures.as_str() != EditorFontLigatures::OFF,
        // Sem seleções: a linha original não pode ser selecionada.
        selections_on_line: None,
        text_direction: None,
        vertical_scrollbar_size: options.vertical_scrollbar_size,
        render_new_line_when_empty: false,
        use_two_cell_fullwidth_characters: options.use_two_cell_fullwidth_characters,
    }));

    let max_char_width = f64::from(output.character_mapping.get_horizontal_offset(output.character_mapping.length as i32));
    let line = RenderedViewLine {
        top: view_line_idx as f64 * options.line_height,
        // Nenhuma mudança de caractere: a linha toda é apagada.
        char_delete: !no_extra && !has_char_changes,
        set_width: options.set_width,
        html: output.html,
        output: RenderLineOutputWithOffset {
            output: RenderLineOutput::new(output.character_mapping, output.contains_foreign_elements),
            offset,
        },
    };
    (line, max_char_width)
}
