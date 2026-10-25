//! Porte de `src/vs/editor/common/viewLayout/viewLineRenderer.ts`: `RenderLineInput`, `RenderLineOutput`,
//! `CharacterMapping`, `DomPosition`, `ForeignElementType` e `renderViewLine` com toda a resolução das partes
//! (tokens, caracteres de controle, espaços em branco visíveis, decorações em linha, quebra de tokens longos,
//! RTL, `fauxIndentLength`, `stopRenderingLineAfter`).
//!
//! A saída HTML que o original escreve no `StringBuilder` é a ESPECIFICAÇÃO deste módulo: o teste do VS Code
//! (`viewLineRenderer.test.ts`) confere essa cadeia caso a caso, então o porte a gera byte a byte igual. Ela
//! define as partes (cada `<span class="...">`), o texto que cada uma carrega (`&nbsp;`, setas de tab, pontos
//! médios, figuras de controle) e, junto com o [`CharacterMapping`], a correspondência entre coluna do modelo,
//! parte e deslocamento horizontal. O desenho nativo (wgpu e rasterizador em software) consome as partes e o
//! mapeamento descritos por essa saída; nenhum HTML é interpretado em tempo de execução.
//!
//! Texto da linha é UTF-16 (`&[u16]`/[`JsString`]), como no restante do porte. Diferenças de representação:
//! - o construtor posicional de `RenderLineInput` vira [`RenderLineInput::new`] sobre [`RenderLineInputOptions`]
//!   (a interface `IRenderLineInputOptions` do original), que já traz os dois últimos parâmetros opcionais;
//! - `ForeignElementType` (enum constante usado como bits) é o módulo [`foreign_element_type`] com constantes
//!   `i32`, no mesmo molde de `line_part_metadata`;
//! - `Uint32Array` do `CharacterMapping` vira `Vec<u32>`; escrita fora do intervalo é ignorada e leitura fora do
//!   intervalo devolve 0, como o `undefined` coagido do typed array;
//! - `String(number)` do JavaScript vira [`js_number_to_string`] (sem notação exponencial, que as larguras em
//!   pixels nunca atingem) e `toFixed(1)` do tamanho de overflow usa aritmética inteira exata;
//! - `_applyInlineDecorations` não recebe o parâmetro `len`, que o original declara e nunca usa.

use std::rc::Rc;

use je_base::JsString;
use je_base::char_code as cc;
use je_base::js_string::{char_code_at as js_char_code_at, substring};
use je_base::nls;
use je_base::strings;

use crate::inline_decorations::InlineDecorationType;
use crate::line_decorations::{LineDecoration, LineDecorationsNormalizer};
use crate::line_part::{LinePart, line_part_metadata};
use crate::line_tokens::IViewLineTokens;
use crate::model::TextDirection;
use crate::offset_range::OffsetRange;
use crate::string_builder::StringBuilder;

/// `RenderWhitespace` (enum constante no original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderWhitespace {
    None = 0,
    Boundary = 1,
    Selection = 2,
    Trailing = 3,
    All = 4,
}

impl RenderWhitespace {
    /// O valor de `editor.renderWhitespace` (`'none' | 'boundary' | 'selection' | 'trailing' | 'all'`); qualquer
    /// outro valor vale `none`.
    pub fn from_option(value: &str) -> Self {
        match value {
            "all" => RenderWhitespace::All,
            "boundary" => RenderWhitespace::Boundary,
            "selection" => RenderWhitespace::Selection,
            "trailing" => RenderWhitespace::Trailing,
            _ => RenderWhitespace::None,
        }
    }
}

/// `ForeignElementType` (enum constante no original, usado como bits).
pub mod foreign_element_type {
    pub const NONE: i32 = 0;
    pub const BEFORE: i32 = 1;
    pub const AFTER: i32 = 2;
}

/// `IRenderLineInputOptions`.
pub struct RenderLineInputOptions {
    pub use_monospace_optimizations: bool,
    pub can_use_halfwidth_rightwards_arrow: bool,
    pub line_content: JsString,
    pub continues_with_wrapped_line: bool,
    pub is_basic_ascii: bool,
    pub contains_rtl: bool,
    pub faux_indent_length: i32,
    pub line_tokens: Rc<dyn IViewLineTokens>,
    pub line_decorations: Vec<LineDecoration>,
    pub tab_size: i32,
    pub start_visible_column: i32,
    pub space_width: f64,
    pub middot_width: f64,
    pub wsmiddot_width: f64,
    /// `-1` desliga o corte.
    pub stop_rendering_line_after: i32,
    /// `'none' | 'boundary' | 'selection' | 'trailing' | 'all'`; qualquer outro valor vale `none`.
    pub render_whitespace: String,
    pub render_control_characters: bool,
    pub font_ligatures: bool,
    pub selections_on_line: Option<Vec<OffsetRange>>,
    pub text_direction: Option<TextDirection>,
    pub vertical_scrollbar_size: f64,
    pub render_new_line_when_empty: bool,
    pub use_two_cell_fullwidth_characters: bool,
}

/// `RenderLineInput`.
#[derive(Clone)]
pub struct RenderLineInput {
    pub use_monospace_optimizations: bool,
    pub can_use_halfwidth_rightwards_arrow: bool,
    pub line_content: JsString,
    pub continues_with_wrapped_line: bool,
    pub is_basic_ascii: bool,
    pub contains_rtl: bool,
    pub faux_indent_length: i32,
    pub line_tokens: Rc<dyn IViewLineTokens>,
    pub line_decorations: Vec<LineDecoration>,
    pub tab_size: i32,
    pub start_visible_column: i32,
    pub space_width: f64,
    pub render_space_width: f64,
    pub render_space_char_code: u16,
    pub stop_rendering_line_after: i32,
    pub render_whitespace: RenderWhitespace,
    pub render_control_characters: bool,
    pub font_ligatures: bool,
    pub text_direction: Option<TextDirection>,
    pub vertical_scrollbar_size: f64,
    /// Definido só quando `render_whitespace` é `selection`. As seleções não se sobrepõem e estão em ordem
    /// de posição na linha.
    pub selections_on_line: Option<Vec<OffsetRange>>,
    /// Ao renderizar uma linha vazia, se deve renderizar uma quebra de linha no lugar.
    pub render_new_line_when_empty: bool,
    /// Se os caracteres de largura total saem centralizados em exatamente duas células.
    pub use_two_cell_fullwidth_characters: bool,
}

impl RenderLineInput {
    /// O construtor do original: ordena as decorações e as seleções e escolhe o caractere (e a largura) que
    /// representa o espaço visível, o que fica mais perto da largura do espaço.
    pub fn new(options: RenderLineInputOptions) -> Self {
        let mut line_decorations = options.line_decorations;
        line_decorations.sort_by(LineDecoration::compare);

        let render_whitespace = RenderWhitespace::from_option(&options.render_whitespace);

        // O comparador do original devolve 1 também para começos iguais; as seleções não se sobrepõem, então
        // começos iguais só ocorrem entre intervalos vazios e a ordem estável basta.
        let selections_on_line = options.selections_on_line.map(|mut selections| {
            selections.sort_by(|a, b| a.start.cmp(&b.start));
            selections
        });

        let wsmiddot_diff = (options.wsmiddot_width - options.space_width).abs();
        let middot_diff = (options.middot_width - options.space_width).abs();
        let (render_space_width, render_space_char_code) = if wsmiddot_diff < middot_diff {
            (options.wsmiddot_width, 0x2E31) // U+2E31 - WORD SEPARATOR MIDDLE DOT
        } else {
            (options.middot_width, 0xB7) // U+00B7 - MIDDLE DOT
        };

        Self {
            use_monospace_optimizations: options.use_monospace_optimizations,
            can_use_halfwidth_rightwards_arrow: options.can_use_halfwidth_rightwards_arrow,
            line_content: options.line_content,
            continues_with_wrapped_line: options.continues_with_wrapped_line,
            is_basic_ascii: options.is_basic_ascii,
            contains_rtl: options.contains_rtl,
            faux_indent_length: options.faux_indent_length,
            line_tokens: options.line_tokens,
            line_decorations,
            tab_size: options.tab_size,
            start_visible_column: options.start_visible_column,
            space_width: options.space_width,
            render_space_width,
            render_space_char_code,
            stop_rendering_line_after: options.stop_rendering_line_after,
            render_whitespace,
            render_control_characters: options.render_control_characters,
            font_ligatures: options.font_ligatures,
            text_direction: options.text_direction,
            vertical_scrollbar_size: options.vertical_scrollbar_size,
            selections_on_line,
            render_new_line_when_empty: options.render_new_line_when_empty,
            use_two_cell_fullwidth_characters: options.use_two_cell_fullwidth_characters,
        }
    }

    /// `isLTR`.
    pub fn is_ltr(&self) -> bool {
        !self.contains_rtl && self.text_direction != Some(TextDirection::Rtl)
    }

    pub fn equals(&self, other: &RenderLineInput) -> bool {
        self.use_monospace_optimizations == other.use_monospace_optimizations
            && self.can_use_halfwidth_rightwards_arrow == other.can_use_halfwidth_rightwards_arrow
            && self.line_content == other.line_content
            && self.continues_with_wrapped_line == other.continues_with_wrapped_line
            && self.is_basic_ascii == other.is_basic_ascii
            && self.contains_rtl == other.contains_rtl
            && self.faux_indent_length == other.faux_indent_length
            && self.tab_size == other.tab_size
            && self.start_visible_column == other.start_visible_column
            && self.space_width == other.space_width
            && self.render_space_width == other.render_space_width
            && self.render_space_char_code == other.render_space_char_code
            && self.stop_rendering_line_after == other.stop_rendering_line_after
            && self.render_whitespace == other.render_whitespace
            && self.render_control_characters == other.render_control_characters
            && self.font_ligatures == other.font_ligatures
            && self.line_decorations == other.line_decorations
            && self.line_tokens.equals(other.line_tokens.as_ref())
            && self.selections_on_line == other.selections_on_line
            && self.text_direction == other.text_direction
            && self.vertical_scrollbar_size == other.vertical_scrollbar_size
            && self.render_new_line_when_empty == other.render_new_line_when_empty
            && self.use_two_cell_fullwidth_characters == other.use_two_cell_fullwidth_characters
    }
}

/// `CharacterMappingConstants`.
mod character_mapping_constants {
    pub const PART_INDEX_MASK: u32 = 0b1111_1111_1111_1111_0000_0000_0000_0000;
    pub const CHAR_INDEX_MASK: u32 = 0b0000_0000_0000_0000_1111_1111_1111_1111;
    pub const PART_INDEX_OFFSET: u32 = 16;
}

/// `DomPosition`: parte e deslocamento dentro dela (em unidades do texto produzido).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomPosition {
    pub part_index: u32,
    pub char_index: u32,
}

impl DomPosition {
    pub fn new(part_index: u32, char_index: u32) -> Self {
        Self { part_index, char_index }
    }
}

/// Mapeamento nos dois sentidos entre o caractere de uma linha e a posição em que ele foi renderizado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterMapping {
    pub length: usize,
    data: Vec<u32>,
    horizontal_offset: Vec<u32>,
}

impl CharacterMapping {
    fn get_part_index(part_data: u32) -> u32 {
        (part_data & character_mapping_constants::PART_INDEX_MASK) >> character_mapping_constants::PART_INDEX_OFFSET
    }

    fn get_char_index(part_data: u32) -> u32 {
        part_data & character_mapping_constants::CHAR_INDEX_MASK
    }

    fn pack(part_index: u32, char_index: u32) -> u32 {
        (part_index << character_mapping_constants::PART_INDEX_OFFSET) | char_index
    }

    /// `partCount` não é usado pelo original (o parâmetro existe só na assinatura).
    pub fn new(length: usize, _part_count: usize) -> Self {
        Self {
            length,
            data: vec![0; length],
            horizontal_offset: vec![0; length],
        }
    }

    pub fn set_column_info(&mut self, column: i32, part_index: u32, char_index: u32, horizontal_offset: u32) {
        let Some(index) = usize::try_from(i64::from(column) - 1).ok().filter(|&index| index < self.length) else {
            // Escrita fora do intervalo de um typed array não faz nada.
            return;
        };
        self.data[index] = CharacterMapping::pack(part_index, char_index);
        self.horizontal_offset[index] = horizontal_offset;
    }

    pub fn get_horizontal_offset(&self, column: i32) -> u32 {
        // Sem caracteres na linha (ou coluna fora do intervalo) devolve 0.
        usize::try_from(i64::from(column) - 1)
            .ok()
            .and_then(|index| self.horizontal_offset.get(index))
            .copied()
            .unwrap_or(0)
    }

    fn char_offset_to_part_data(&self, char_offset: i32) -> u32 {
        if self.length == 0 {
            return 0;
        }
        if char_offset < 0 {
            return self.data[0];
        }
        if char_offset as usize >= self.length {
            return self.data[self.length - 1];
        }
        self.data[char_offset as usize]
    }

    pub fn get_dom_position(&self, column: i32) -> DomPosition {
        let part_data = self.char_offset_to_part_data(column - 1);
        DomPosition::new(CharacterMapping::get_part_index(part_data), CharacterMapping::get_char_index(part_data))
    }

    pub fn get_column(&self, dom_position: &DomPosition, part_length: u32) -> i32 {
        let char_offset = self.part_data_to_char_offset(dom_position.part_index, part_length, dom_position.char_index);
        char_offset as i32 + 1
    }

    fn part_data_to_char_offset(&self, part_index: u32, part_length: u32, char_index: u32) -> usize {
        if self.length == 0 {
            return 0;
        }

        let search_entry = CharacterMapping::pack(part_index, char_index);

        let mut min = 0usize;
        let mut max = self.length - 1;
        while min + 1 < max {
            let mid = (min + max) >> 1;
            let mid_entry = self.data[mid];
            if mid_entry == search_entry {
                return mid;
            } else if mid_entry > search_entry {
                max = mid;
            } else {
                min = mid;
            }
        }

        if min == max {
            return min;
        }

        let min_entry = self.data[min];
        let max_entry = self.data[max];

        if min_entry == search_entry {
            return min;
        }
        if max_entry == search_entry {
            return max;
        }

        let min_part_index = CharacterMapping::get_part_index(min_entry);
        let min_char_index = CharacterMapping::get_char_index(min_entry);

        let max_part_index = CharacterMapping::get_part_index(max_entry);
        let max_char_index = if min_part_index != max_part_index {
            // Entre duas partes.
            part_length
        } else {
            CharacterMapping::get_char_index(max_entry)
        };

        let min_entry_distance = i64::from(char_index) - i64::from(min_char_index);
        let max_entry_distance = i64::from(max_char_index) - i64::from(char_index);

        if min_entry_distance <= max_entry_distance {
            return min;
        }
        max
    }

    /// `inflate()`: uma tupla `(parte, caractere, deslocamento horizontal)` por coluna.
    pub fn inflate(&self) -> Vec<(u32, u32, u32)> {
        (0..self.length)
            .map(|i| {
                let part_data = self.data[i];
                (
                    CharacterMapping::get_part_index(part_data),
                    CharacterMapping::get_char_index(part_data),
                    self.horizontal_offset[i],
                )
            })
            .collect()
    }
}

/// `RenderLineOutput`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderLineOutput {
    pub character_mapping: CharacterMapping,
    /// Bits de [`foreign_element_type`].
    pub contains_foreign_elements: i32,
}

impl RenderLineOutput {
    pub fn new(character_mapping: CharacterMapping, contains_foreign_elements: i32) -> Self {
        Self { character_mapping, contains_foreign_elements }
    }
}

/// `RenderLineOutput2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderLineOutput2 {
    pub character_mapping: CharacterMapping,
    pub html: JsString,
    pub contains_foreign_elements: i32,
}

/// Escreve um texto Rust no `StringBuilder` (que trabalha em unidades UTF-16).
fn append_str(sb: &mut StringBuilder, text: &str) {
    sb.append_string(&text.encode_utf16().collect::<Vec<u16>>());
}

/// `str.charCodeAt(index)`: `None` é o `NaN` do original.
fn char_code_at(line_content: &[u16], index: i32) -> Option<u16> {
    js_char_code_at(line_content, i64::from(index))
}

/// `String(number)` do JavaScript para os valores finitos que as larguras em pixels assumem: o menor texto
/// que lê de volta o mesmo número, sem expoente.
pub fn js_number_to_string(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        (if value > 0.0 { "Infinity" } else { "-Infinity" }).to_owned()
    } else if value == 0.0 {
        "0".to_owned()
    } else {
        value.to_string()
    }
}

pub fn render_view_line(input: &RenderLineInput, sb: &mut StringBuilder) -> RenderLineOutput {
    if input.line_content.is_empty() {
        if !input.line_decorations.is_empty() {
            // A linha é vazia, mas tem decorações em linha.
            append_str(sb, "<span>");

            let mut before_count = 0u32;
            let mut after_count = 0usize;
            let mut contains_foreign_elements = foreign_element_type::NONE;
            for line_decoration in &input.line_decorations {
                if matches!(
                    line_decoration.r#type,
                    InlineDecorationType::Before | InlineDecorationType::After | InlineDecorationType::WidthOnly
                ) {
                    append_str(sb, "<span class=\"");
                    sb.append_string(line_decoration.class_name.units());
                    append_str(sb, "\"></span>");

                    if line_decoration.r#type == InlineDecorationType::After {
                        contains_foreign_elements |= foreign_element_type::AFTER;
                        after_count += 1;
                    } else {
                        // Decorações só de largura ficam antes do caractere da coluna, como as de conteúdo
                        // anterior.
                        contains_foreign_elements |= foreign_element_type::BEFORE;
                        before_count += 1;
                    }
                }
            }

            append_str(sb, "</span>");

            let mut character_mapping = CharacterMapping::new(1, before_count as usize + after_count);
            character_mapping.set_column_info(1, before_count, 0, 0);

            return RenderLineOutput::new(character_mapping, contains_foreign_elements);
        }

        // Linha completamente vazia.
        if input.render_new_line_when_empty {
            append_str(sb, "<span><span>\n</span></span>");
        } else {
            append_str(sb, "<span><span></span></span>");
        }
        return RenderLineOutput::new(CharacterMapping::new(0, 0), foreign_element_type::NONE);
    }

    render_line(&resolve_render_line_input(input), sb)
}

pub fn render_view_line2(input: &RenderLineInput) -> RenderLineOutput2 {
    let mut sb = StringBuilder::new(10000);
    let out = render_view_line(input, &mut sb);
    RenderLineOutput2 {
        character_mapping: out.character_mapping,
        html: sb.build(),
        contains_foreign_elements: out.contains_foreign_elements,
    }
}

struct ResolvedRenderLineInput<'a> {
    font_is_monospace: bool,
    can_use_halfwidth_rightwards_arrow: bool,
    line_content: &'a [u16],
    len: i32,
    is_overflowing: bool,
    overflowing_char_count: i32,
    parts: Vec<LinePart>,
    contains_foreign_elements: i32,
    faux_indent_length: i32,
    tab_size: i32,
    start_visible_column: i32,
    space_width: f64,
    render_space_char_code: u16,
    render_whitespace: RenderWhitespace,
    render_control_characters: bool,
    use_two_cell_fullwidth_characters: bool,
}

fn resolve_render_line_input(input: &RenderLineInput) -> ResolvedRenderLineInput<'_> {
    let line_content = input.line_content.units();
    let content_len = line_content.len() as i32;

    let (is_overflowing, overflowing_char_count, len) =
        if input.stop_rendering_line_after != -1 && input.stop_rendering_line_after < content_len {
            (true, content_len - input.stop_rendering_line_after, input.stop_rendering_line_after)
        } else {
            (false, 0, content_len)
        };

    let mut tokens = transform_and_remove_overflowing(
        line_content,
        input.contains_rtl,
        input.line_tokens.as_ref(),
        input.faux_indent_length,
        len,
    );
    if input.render_control_characters && !input.is_basic_ascii {
        // `extract_control_characters` antes de somar as partes (possivelmente vazias) das decorações em
        // linha: ela remove partes vazias.
        tokens = extract_control_characters(line_content, &tokens);
    }
    if input.render_whitespace == RenderWhitespace::All
        || input.render_whitespace == RenderWhitespace::Boundary
        || (input.render_whitespace == RenderWhitespace::Selection && input.selections_on_line.is_some())
        || (input.render_whitespace == RenderWhitespace::Trailing && !input.continues_with_wrapped_line)
    {
        tokens = apply_render_whitespace(input, line_content, len, &tokens);
    }
    let mut contains_foreign_elements = foreign_element_type::NONE;
    if !input.line_decorations.is_empty() {
        for line_decoration in &input.line_decorations {
            match line_decoration.r#type {
                // Finge que há elementos estrangeiros, embora não seja 100% exato.
                InlineDecorationType::RegularAffectingLetterSpacing => {
                    contains_foreign_elements |= foreign_element_type::BEFORE;
                }
                InlineDecorationType::Before => contains_foreign_elements |= foreign_element_type::BEFORE,
                InlineDecorationType::After => contains_foreign_elements |= foreign_element_type::AFTER,
                // Idem: finge, ver acima.
                InlineDecorationType::WidthOnly => contains_foreign_elements |= foreign_element_type::BEFORE,
                InlineDecorationType::Regular => {}
            }
        }
        tokens = apply_inline_decorations(line_content, &tokens, &input.line_decorations);
    }
    if !input.contains_rtl {
        // Texto RTL nunca pode ser partido, estraga a renderização.
        tokens = split_large_tokens(line_content, &tokens, !input.is_basic_ascii || input.font_ligatures);
    } else {
        // Parte o primeiro token se ele tem espaço à esquerda e texto RTL.
        tokens = split_leading_whitespace_from_rtl(line_content, tokens);
    }
    if input.use_two_cell_fullwidth_characters {
        tokens = split_full_width_characters(line_content, &tokens);
    }

    ResolvedRenderLineInput {
        font_is_monospace: input.use_monospace_optimizations,
        can_use_halfwidth_rightwards_arrow: input.can_use_halfwidth_rightwards_arrow,
        line_content,
        len,
        is_overflowing,
        overflowing_char_count,
        parts: tokens,
        contains_foreign_elements,
        faux_indent_length: input.faux_indent_length,
        tab_size: input.tab_size,
        start_visible_column: input.start_visible_column,
        space_width: input.space_width,
        render_space_char_code: input.render_space_char_code,
        render_whitespace: input.render_whitespace,
        render_control_characters: input.render_control_characters,
        use_two_cell_fullwidth_characters: input.use_two_cell_fullwidth_characters,
    }
}

/// Parte as partes de modo que cada caractere de largura total fique numa parte só dele.
fn split_full_width_characters(line_content: &[u16], parts: &[LinePart]) -> Vec<LinePart> {
    let mut result = Vec::new();
    let mut start_offset = 0;
    for part in parts {
        let mut did_split = false;
        let mut offset = start_offset;
        while offset < part.end_index {
            if !char_code_at(line_content, offset).is_some_and(strings::is_full_width_character) {
                offset += 1;
                continue;
            }
            if offset > start_offset {
                result.push(LinePart::new(offset, part.r#type.clone(), part.metadata, part.contains_rtl));
            }
            result.push(LinePart::new(offset + 1, part.r#type.clone(), part.metadata, part.contains_rtl));
            start_offset = offset + 1;
            did_split = true;
            offset += 1;
        }
        if !did_split || part.end_index > start_offset {
            result.push(part.clone());
        }
        start_offset = part.end_index;
    }
    result
}

/// Na fase de renderização os caracteres sempre são percorridos até `token.endIndex`. Garante que todos os
/// tokens terminem antes de `len` e que o último termine exatamente em `len`.
fn transform_and_remove_overflowing(
    line_content: &[u16],
    line_contains_rtl: bool,
    tokens: &dyn IViewLineTokens,
    faux_indent_length: i32,
    len: i32,
) -> Vec<LinePart> {
    let mut result = Vec::new();

    // A parte do recuo falso da linha não tem tipo de token.
    if faux_indent_length > 0 {
        result.push(LinePart::new(faux_indent_length, "", 0, false));
    }
    let mut start_offset = faux_indent_length;
    for token_index in 0..tokens.get_count() {
        let end_index = tokens.get_end_offset(token_index);
        if end_index <= faux_indent_length {
            // A parte do recuo falso da linha não tem tipo de token.
            continue;
        }
        let r#type = tokens.get_class_name(token_index);
        if end_index >= len {
            let token_contains_rtl = line_contains_rtl
                && strings::contains_rtl(substring(line_content, i64::from(start_offset), i64::from(len)));
            result.push(LinePart::new(len, r#type, 0, token_contains_rtl));
            break;
        }
        let token_contains_rtl = line_contains_rtl
            && strings::contains_rtl(substring(line_content, i64::from(start_offset), i64::from(end_index)));
        result.push(LinePart::new(end_index, r#type, 0, token_contains_rtl));
        start_offset = end_index;
    }

    result
}

/// `Constants.LongToken`.
const LONG_TOKEN: i32 = 50;

/// Ver https://github.com/microsoft/vscode/issues/6885. Spans muito grandes deixam muito lenta a leitura das
/// posições dos caracteres, então aqui tentamos evitá-los.
fn split_large_tokens(line_content: &[u16], tokens: &[LinePart], only_at_spaces: bool) -> Vec<LinePart> {
    let mut last_token_end_index = 0;
    let mut result = Vec::new();

    if only_at_spaces {
        // Parte só nos espaços, então é preciso percorrer cada caractere.
        for token in tokens {
            let token_end_index = token.end_index;
            if last_token_end_index + LONG_TOKEN < token_end_index {
                let mut last_space_offset = -1;
                let mut curr_token_start = last_token_end_index;
                for j in last_token_end_index..token_end_index {
                    if char_code_at(line_content, j) == Some(cc::Space) {
                        last_space_offset = j;
                    }
                    if last_space_offset != -1 && j - curr_token_start >= LONG_TOKEN {
                        // Parte em `last_space_offset + 1`.
                        result.push(LinePart::new(
                            last_space_offset + 1,
                            token.r#type.clone(),
                            token.metadata,
                            token.contains_rtl,
                        ));
                        curr_token_start = last_space_offset + 1;
                        last_space_offset = -1;
                    }
                }
                if curr_token_start != token_end_index {
                    result.push(LinePart::new(token_end_index, token.r#type.clone(), token.metadata, token.contains_rtl));
                }
            } else {
                result.push(token.clone());
            }

            last_token_end_index = token_end_index;
        }
    } else {
        // Parte em qualquer lugar, então não é preciso percorrer cada caractere.
        for token in tokens {
            let token_end_index = token.end_index;
            let diff = token_end_index - last_token_end_index;
            if diff > LONG_TOKEN {
                let pieces_count = (diff + LONG_TOKEN - 1) / LONG_TOKEN;
                for j in 1..pieces_count {
                    let piece_end_index = last_token_end_index + (j * LONG_TOKEN);
                    result.push(LinePart::new(piece_end_index, token.r#type.clone(), token.metadata, token.contains_rtl));
                }
                result.push(LinePart::new(token_end_index, token.r#type.clone(), token.metadata, token.contains_rtl));
            } else {
                result.push(token.clone());
            }
            last_token_end_index = token_end_index;
        }
    }

    result
}

/// Separa o espaço à esquerda do primeiro token se ele contém texto RTL.
fn split_leading_whitespace_from_rtl(line_content: &[u16], tokens: Vec<LinePart>) -> Vec<LinePart> {
    let Some(first_token) = tokens.first() else {
        return tokens;
    };
    if !first_token.contains_rtl {
        return tokens;
    }

    // Confere se o primeiro token começa com espaço em branco.
    let first_token_end_index = first_token.end_index;
    let mut first_non_whitespace_index = 0;
    for i in 0..first_token_end_index {
        let char_code = char_code_at(line_content, i);
        if char_code != Some(cc::Space) && char_code != Some(cc::Tab) {
            first_non_whitespace_index = i;
            break;
        }
    }

    if first_non_whitespace_index == 0 {
        // Sem espaço à esquerda.
        return tokens;
    }

    // Parte o primeiro token em espaço à esquerda e o resto.
    let mut result = Vec::with_capacity(tokens.len() + 1);
    result.push(LinePart::new(first_non_whitespace_index, first_token.r#type.clone(), first_token.metadata, false));
    result.push(LinePart::new(
        first_token_end_index,
        first_token.r#type.clone(),
        first_token.metadata,
        first_token.contains_rtl,
    ));

    // Acrescenta os demais tokens.
    result.extend(tokens[1..].iter().cloned());

    result
}

fn is_control_character(char_code: u16) -> bool {
    if char_code < 32 {
        return char_code != cc::Tab;
    }
    if char_code == 127 {
        // DEL
        return true;
    }

    // Caracteres de formatação direcional Unicode:
    // LRE U+202A, RLE U+202B, PDF U+202C, LRO U+202D, RLO U+202E, LRI U+2066, RLI U+2067, FSI U+2068,
    // PDI U+2069, LRM U+200E, RLM U+200F, ALM U+061C.
    (0x202A..=0x202E).contains(&char_code)
        || (0x2066..=0x2069).contains(&char_code)
        || (0x200E..=0x200F).contains(&char_code)
        || char_code == 0x061C
}

fn extract_control_characters(line_content: &[u16], tokens: &[LinePart]) -> Vec<LinePart> {
    let mut result = Vec::new();
    let mut last_line_part_end_index = 0;
    let mut char_offset = 0;
    for token in tokens {
        let token_end_index = token.end_index;
        while char_offset < token_end_index {
            if char_code_at(line_content, char_offset).is_some_and(is_control_character) {
                if char_offset > last_line_part_end_index {
                    // Emite a parte anterior se ela tem texto.
                    result.push(LinePart::new(char_offset, token.r#type.clone(), token.metadata, token.contains_rtl));
                }
                last_line_part_end_index = char_offset + 1;
                result.push(LinePart::new(char_offset + 1, "mtkcontrol", token.metadata, false));
            }
            char_offset += 1;
        }
        if char_offset > last_line_part_end_index {
            // Emite a parte anterior se ela tem texto.
            last_line_part_end_index = token_end_index;
            result.push(LinePart::new(token_end_index, token.r#type.clone(), token.metadata, token.contains_rtl));
        }
    }
    result
}

/// O espaço em branco é renderizado "trocando" tokens por um tipo especial `mtkw`, reconhecido depois na fase
/// de renderização. Além disso, cria-se um token por recuo visual, porque em algumas fontes os glifos que
/// desenham o espaço (seta ou ponto médio) não têm a mesma largura que o `&nbsp;`. A fase de renderização gera
/// `style="width:..."` para esses tokens.
fn apply_render_whitespace(input: &RenderLineInput, line_content: &[u16], len: i32, tokens: &[LinePart]) -> Vec<LinePart> {
    let continues_with_wrapped_line = input.continues_with_wrapped_line;
    let faux_indent_length = input.faux_indent_length;
    let tab_size = input.tab_size;
    let start_visible_column = input.start_visible_column;
    let use_monospace_optimizations = input.use_monospace_optimizations;
    let selections = input.selections_on_line.as_deref();
    let only_boundary = input.render_whitespace == RenderWhitespace::Boundary;
    let only_trailing = input.render_whitespace == RenderWhitespace::Trailing;
    let generate_line_part_for_each_whitespace = input.render_space_width != input.space_width;

    let mut result: Vec<LinePart> = Vec::new();
    let mut token_index = 0;
    let mut token_type = tokens[token_index].r#type.clone();
    let mut token_contains_rtl = tokens[token_index].contains_rtl;
    let mut token_end_index = tokens[token_index].end_index;
    let tokens_length = tokens.len();

    let mut line_is_empty_or_whitespace = false;
    let mut first_non_whitespace_index =
        strings::first_non_whitespace_index(line_content).map_or(-1, |index| index as i32);
    let last_non_whitespace_index: i32;
    if first_non_whitespace_index == -1 {
        line_is_empty_or_whitespace = true;
        first_non_whitespace_index = len;
        last_non_whitespace_index = len;
    } else {
        last_non_whitespace_index = strings::last_non_whitespace_index(line_content, line_content.len() as i64 - 1)
            .map_or(-1, |index| index as i32);
    }

    let mut was_in_whitespace = false;
    let mut current_selection_index = 0;
    let mut current_selection = selections.and_then(|s| s.get(current_selection_index).copied());
    let mut tmp_indent = start_visible_column % tab_size;
    for char_index in faux_indent_length..len {
        // `char_index < len <= line_content.len()`.
        let ch_code = line_content[char_index as usize];

        if current_selection.is_some_and(|selection| selection.end_exclusive <= char_index) {
            current_selection_index += 1;
            current_selection = selections.and_then(|s| s.get(current_selection_index).copied());
        }

        let mut is_in_whitespace: bool;
        if char_index < first_non_whitespace_index || char_index > last_non_whitespace_index {
            // No espaço à esquerda ou à direita.
            is_in_whitespace = true;
        } else if ch_code == cc::Tab {
            // O tab é renderizado tanto em `all` quanto em `boundary`.
            is_in_whitespace = true;
        } else if ch_code == cc::Space {
            // Achou um espaço.
            if only_boundary {
                // Renderizando só o espaço de fronteira.
                if was_in_whitespace {
                    is_in_whitespace = true;
                } else {
                    let next_ch_code = if char_index + 1 < len {
                        line_content[(char_index + 1) as usize]
                    } else {
                        cc::Null
                    };
                    is_in_whitespace = next_ch_code == cc::Space || next_ch_code == cc::Tab;
                }
            } else {
                is_in_whitespace = true;
            }
        } else {
            is_in_whitespace = false;
        }

        // Renderizando espaço na seleção: confere se o `char_index` cai dentro de uma seleção.
        if is_in_whitespace && selections.is_some() {
            is_in_whitespace = current_selection
                .is_some_and(|selection| selection.start <= char_index && char_index < selection.end_exclusive);
        }

        // Renderizando só o espaço à direita: confere se o `char_index` aponta para ele.
        if is_in_whitespace && only_trailing {
            is_in_whitespace = line_is_empty_or_whitespace || char_index > last_non_whitespace_index;
        }

        if is_in_whitespace && token_contains_rtl {
            // Se o token tem texto RTL, partir em várias partes para renderizar o espaço pode afetar o layout
            // bidi do navegador.
            //
            // O espaço desses tokens só é renderizado quando é o espaço à esquerda ou à direita da linha, o
            // que não afeta o layout bidi.
            if char_index >= first_non_whitespace_index && char_index <= last_non_whitespace_index {
                is_in_whitespace = false;
            }
        }

        if was_in_whitespace {
            // Estava num token de espaço.
            if !is_in_whitespace || (!use_monospace_optimizations && tmp_indent >= tab_size) {
                // Saindo do token de espaço ou entrando num novo recuo.
                if generate_line_part_for_each_whitespace {
                    let last_end_index = result.last().map_or(faux_indent_length, |part| part.end_index);
                    for i in (last_end_index + 1)..=char_index {
                        result.push(LinePart::new(i, "mtkw", line_part_metadata::IS_WHITESPACE, false));
                    }
                } else {
                    result.push(LinePart::new(char_index, "mtkw", line_part_metadata::IS_WHITESPACE, false));
                }
                tmp_indent %= tab_size;
            }
        } else {
            // Estava num token comum.
            if char_index == token_end_index || (is_in_whitespace && char_index > faux_indent_length) {
                result.push(LinePart::new(char_index, token_type.clone(), 0, token_contains_rtl));
                tmp_indent %= tab_size;
            }
        }

        if ch_code == cc::Tab {
            tmp_indent = tab_size;
        } else if strings::is_full_width_character(ch_code) {
            tmp_indent += 2;
        } else {
            tmp_indent += 1;
        }

        was_in_whitespace = is_in_whitespace;

        while char_index == token_end_index {
            token_index += 1;
            if token_index < tokens_length {
                token_type = tokens[token_index].r#type.clone();
                token_contains_rtl = tokens[token_index].contains_rtl;
                token_end_index = tokens[token_index].end_index;
            } else {
                break;
            }
        }
    }

    let mut generate_whitespace = false;
    if was_in_whitespace {
        // Estava num token de espaço.
        if continues_with_wrapped_line && only_boundary {
            let last_char_code = if len > 0 { line_content[(len - 1) as usize] } else { cc::Null };
            let prev_char_code = if len > 1 { line_content[(len - 2) as usize] } else { cc::Null };
            let is_single_trailing_space =
                last_char_code == cc::Space && (prev_char_code != cc::Space && prev_char_code != cc::Tab);
            if !is_single_trailing_space {
                generate_whitespace = true;
            }
        } else {
            generate_whitespace = true;
        }
    }

    if generate_whitespace {
        if generate_line_part_for_each_whitespace {
            let last_end_index = result.last().map_or(faux_indent_length, |part| part.end_index);
            for i in (last_end_index + 1)..=len {
                result.push(LinePart::new(i, "mtkw", line_part_metadata::IS_WHITESPACE, false));
            }
        } else {
            result.push(LinePart::new(len, "mtkw", line_part_metadata::IS_WHITESPACE, false));
        }
    } else {
        result.push(LinePart::new(len, token_type, 0, token_contains_rtl));
    }

    result
}

/// As decorações em linha são "mescladas" sobre os tokens. É preciso cuidado quando várias decorações estão
/// em jogo e se sobrepõem.
fn apply_inline_decorations(line_content: &[u16], tokens: &[LinePart], line_decorations_in: &[LineDecoration]) -> Vec<LinePart> {
    // O original ordena o array recebido; ordenado, o `sort` estável não muda nada.
    let sorted_copy;
    let sorted: &[LineDecoration] =
        if line_decorations_in.is_sorted_by(|a, b| LineDecoration::compare(a, b) != std::cmp::Ordering::Greater) {
            line_decorations_in
        } else {
            let mut copy = line_decorations_in.to_vec();
            copy.sort_by(LineDecoration::compare);
            sorted_copy = copy;
            &sorted_copy
        };
    let line_decorations = LineDecorationsNormalizer::normalize(line_content, sorted);
    let line_decorations_len = line_decorations.len();

    let mut line_decoration_index = 0;
    let mut result = Vec::new();
    let mut last_result_end_index = 0;
    for token in tokens {
        let token_end_index = token.end_index;
        let token_type = &token.r#type;
        let token_metadata = token.metadata;
        let token_contains_rtl = token.contains_rtl;

        while line_decoration_index < line_decorations_len
            && line_decorations[line_decoration_index].start_offset < token_end_index
        {
            let line_decoration = &line_decorations[line_decoration_index];

            if line_decoration.start_offset > last_result_end_index {
                last_result_end_index = line_decoration.start_offset;
                result.push(LinePart::new(last_result_end_index, token_type.clone(), token_metadata, token_contains_rtl));
            }

            let decorated_type = format!("{} {}", token_type, line_decoration.class_name.to_string_lossy());
            if line_decoration.end_offset + 1 <= token_end_index {
                // Esta decoração termina antes de o token terminar.
                last_result_end_index = line_decoration.end_offset + 1;
                result.push(LinePart::new(
                    last_result_end_index,
                    decorated_type,
                    token_metadata | line_decoration.metadata,
                    token_contains_rtl,
                ));
                line_decoration_index += 1;
            } else {
                // Esta decoração continua no próximo token.
                last_result_end_index = token_end_index;
                result.push(LinePart::new(
                    last_result_end_index,
                    decorated_type,
                    token_metadata | line_decoration.metadata,
                    token_contains_rtl,
                ));
                break;
            }
        }

        if token_end_index > last_result_end_index {
            last_result_end_index = token_end_index;
            result.push(LinePart::new(last_result_end_index, token_type.clone(), token_metadata, token_contains_rtl));
        }
    }

    let last_token_end_index = tokens[tokens.len() - 1].end_index;
    if line_decoration_index < line_decorations_len
        && line_decorations[line_decoration_index].start_offset == last_token_end_index
    {
        while line_decoration_index < line_decorations_len
            && line_decorations[line_decoration_index].start_offset == last_token_end_index
        {
            let line_decoration = &line_decorations[line_decoration_index];
            result.push(LinePart::new(
                last_result_end_index,
                line_decoration.class_name.to_string_lossy(),
                line_decoration.metadata,
                false,
            ));
            line_decoration_index += 1;
        }
    }

    result
}

/// Esta função de propósito não é dividida em várias (o original faz isso por desempenho, para inferência de
/// tipos em tempo de execução). Todos os dados necessários chegam resolvidos, sem outras chamadas.
fn render_line(input: &ResolvedRenderLineInput, sb: &mut StringBuilder) -> RenderLineOutput {
    let font_is_monospace = input.font_is_monospace;
    let can_use_halfwidth_rightwards_arrow = input.can_use_halfwidth_rightwards_arrow;
    let contains_foreign_elements = input.contains_foreign_elements;
    let line_content = input.line_content;
    let len = input.len;
    let parts = &input.parts;
    let faux_indent_length = input.faux_indent_length;
    let tab_size = input.tab_size;
    let start_visible_column = input.start_visible_column;
    let space_width = input.space_width;
    let render_space_char_code = input.render_space_char_code;
    let render_whitespace = input.render_whitespace;
    let render_control_characters = input.render_control_characters;
    let use_two_cell_fullwidth_characters = input.use_two_cell_fullwidth_characters;
    let full_width_character_width = 2.0 * input.space_width;

    let mut character_mapping = CharacterMapping::new(len as usize + 1, parts.len());
    let mut last_character_mapping_defined = false;

    let mut char_index: i32 = 0;
    let mut visible_column = start_visible_column;
    // O deslocamento do caractere na parte atual.
    let mut char_offset_in_part: i32 = 0;
    // A posição horizontal do caractere em caracteres, relativa ao começo da linha.
    let mut char_horizontal_offset: i32 = 0;

    let mut part_displacement: u32 = 0;

    append_str(sb, "<span>");

    for (part_position, part) in parts.iter().enumerate() {
        let part_index = part_position as u32;
        let part_end_index = part.end_index;
        let part_type = &part.r#type;
        let part_contains_rtl = part.contains_rtl;
        let part_renders_whitespace = render_whitespace != RenderWhitespace::None && part.is_whitespace();
        let part_renders_whitespace_with_width = part_renders_whitespace
            && !font_is_monospace
            && (part_type == "mtkw" /* só espaço */ || contains_foreign_elements == foreign_element_type::NONE);
        let part_is_empty_and_has_pseudo_after = char_index == part_end_index && part.is_pseudo_after();
        // Um caractere de largura total fica centralizado em duas células, a menos que a parte já precise do
        // atributo style para o isolamento bidi.
        let part_is_centered_full_width = !part_contains_rtl
            && use_two_cell_fullwidth_characters
            && char_code_at(line_content, char_index).is_some_and(strings::is_full_width_character);
        char_offset_in_part = 0;

        append_str(sb, "<span ");
        if part_contains_rtl {
            append_str(sb, "style=\"unicode-bidi:isolate\" ");
        } else if part_is_centered_full_width {
            append_str(sb, "style=\"width:");
            append_str(sb, &js_number_to_string(full_width_character_width));
            append_str(sb, "px\" ");
        }
        append_str(sb, "class=\"");
        append_str(sb, if part_renders_whitespace_with_width { "mtkz" } else { part_type });
        if part_is_centered_full_width {
            append_str(sb, " mtkfullwidth");
        }
        sb.append_ascii_char_code(cc::DoubleQuote);

        if part_renders_whitespace {
            let mut part_width: i32 = 0;
            {
                let mut inner_char_index = char_index;
                let mut inner_visible_column = visible_column;

                while inner_char_index < part_end_index {
                    let char_code = char_code_at(line_content, inner_char_index);
                    let char_width = if char_code == Some(cc::Tab) {
                        tab_size - (inner_visible_column % tab_size)
                    } else {
                        1
                    };
                    part_width += char_width;
                    if inner_char_index >= faux_indent_length {
                        inner_visible_column += char_width;
                    }
                    inner_char_index += 1;
                }
            }

            if part_renders_whitespace_with_width {
                append_str(sb, " style=\"width:");
                append_str(sb, &js_number_to_string(space_width * f64::from(part_width)));
                append_str(sb, "px\"");
            }
            sb.append_ascii_char_code(cc::GreaterThan);

            while char_index < part_end_index {
                character_mapping.set_column_info(
                    char_index + 1,
                    part_index - part_displacement,
                    char_offset_in_part as u32,
                    char_horizontal_offset as u32,
                );
                part_displacement = 0;
                let char_code = char_code_at(line_content, char_index);

                let produced_characters: i32;
                let char_width: i32;

                if char_code == Some(cc::Tab) {
                    produced_characters = tab_size - (visible_column % tab_size);
                    char_width = produced_characters;

                    if !can_use_halfwidth_rightwards_arrow || char_width > 1 {
                        sb.append_char_code(0x2192); // RIGHTWARDS ARROW
                    } else {
                        sb.append_char_code(0xFFEB); // HALFWIDTH RIGHTWARDS ARROW
                    }
                    for _space in 2..=char_width {
                        sb.append_char_code(0xA0); // &nbsp;
                    }
                } else {
                    // Tem de ser `CharCode.Space`.
                    produced_characters = 2;
                    char_width = 1;

                    sb.append_char_code(render_space_char_code); // &middot; ou ponto médio separador de palavra
                    sb.append_char_code(0x200C); // ZERO WIDTH NON-JOINER
                }

                char_offset_in_part += produced_characters;
                char_horizontal_offset += char_width;
                if char_index >= faux_indent_length {
                    visible_column += char_width;
                }
                char_index += 1;
            }
        } else {
            sb.append_ascii_char_code(cc::GreaterThan);

            while char_index < part_end_index {
                character_mapping.set_column_info(
                    char_index + 1,
                    part_index - part_displacement,
                    char_offset_in_part as u32,
                    char_horizontal_offset as u32,
                );
                part_displacement = 0;
                // O `NaN` do original só apareceria com `faux_indent_length` além do texto.
                let char_code = char_code_at(line_content, char_index).unwrap_or(cc::Null);

                let mut produced_characters: i32 = 1;
                let mut char_width: i32 = 1;

                match char_code {
                    cc::Tab => {
                        produced_characters = tab_size - (visible_column % tab_size);
                        char_width = produced_characters;
                        for _space in 1..=produced_characters {
                            sb.append_char_code(0xA0); // &nbsp;
                        }
                    }

                    cc::Space => sb.append_char_code(0xA0), // &nbsp;

                    cc::LessThan => append_str(sb, "&lt;"),

                    cc::GreaterThan => append_str(sb, "&gt;"),

                    cc::Ampersand => append_str(sb, "&amp;"),

                    cc::Null => {
                        if render_control_characters {
                            // Ver https://unicode-table.com/en/blocks/control-pictures/
                            sb.append_char_code(9216);
                        } else {
                            append_str(sb, "&#00;");
                        }
                    }

                    cc::UTF8_BOM | cc::LINE_SEPARATOR | cc::PARAGRAPH_SEPARATOR | cc::NEXT_LINE => {
                        sb.append_char_code(0xFFFD);
                    }

                    _ => {
                        if strings::is_full_width_character(char_code) {
                            char_width += 1;
                        }
                        // Ver https://unicode-table.com/en/blocks/control-pictures/
                        if render_control_characters && char_code < 32 {
                            sb.append_char_code(9216 + char_code);
                        } else if render_control_characters && char_code == 127 {
                            // DEL
                            sb.append_char_code(9249);
                        } else if render_control_characters && is_control_character(char_code) {
                            append_str(sb, "[U+");
                            append_str(sb, &to_4_char_hex(char_code));
                            append_str(sb, "]");
                            produced_characters = 8;
                            char_width = produced_characters;
                        } else {
                            sb.append_char_code(char_code);
                        }
                    }
                }

                char_offset_in_part += produced_characters;
                char_horizontal_offset += char_width;
                if char_index >= faux_indent_length {
                    visible_column += char_width;
                }
                char_index += 1;
            }
        }

        if part_is_empty_and_has_pseudo_after {
            part_displacement += 1;
        } else {
            part_displacement = 0;
        }

        if char_index >= len && !last_character_mapping_defined && part.is_pseudo_after() {
            last_character_mapping_defined = true;
            character_mapping.set_column_info(
                char_index + 1,
                part_index,
                char_offset_in_part as u32,
                char_horizontal_offset as u32,
            );
        }

        append_str(sb, "</span>");
    }

    if !last_character_mapping_defined {
        // Ao obter os retângulos do último caractere, o intervalo de texto fica no fim do span, em vez de no
        // começo do próximo.
        character_mapping.set_column_info(
            len + 1,
            (parts.len() as u32).wrapping_sub(1),
            char_offset_in_part as u32,
            char_horizontal_offset as u32,
        );
    }

    if input.is_overflowing {
        append_str(sb, "<span class=\"mtkoverflow\">");
        append_str(
            sb,
            &nls::localize("showMore", "Show more ({0})", &[&render_overflowing_char_count(input.overflowing_char_count)]),
        );
        append_str(sb, "</span>");
    }

    append_str(sb, "</span>");

    RenderLineOutput::new(character_mapping, contains_foreign_elements)
}

fn to_4_char_hex(n: u16) -> String {
    format!("{n:04X}")
}

/// `(n / divisor).toFixed(1)` para `divisor` potência de dois: a divisão é exata em ponto flutuante, e o
/// `toFixed` escolhe o maior `n` quando há empate, ou seja, arredonda meio para cima. Feito em inteiros.
fn to_fixed_1_of_ratio(n: u64, divisor: u64) -> String {
    let tenths = (n * 20 + divisor) / (2 * divisor);
    format!("{}.{}", tenths / 10, tenths % 10)
}

fn render_overflowing_char_count(n: i32) -> String {
    let n = u64::try_from(n).unwrap_or(0);
    if n < 1024 {
        return nls::localize("overflow.chars", "{0} chars", &[&n.to_string()]);
    }
    if n < 1024 * 1024 {
        return format!("{} KB", to_fixed_1_of_ratio(n, 1024));
    }
    format!("{} MB", to_fixed_1_of_ratio(n, 1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mapeamento de 3 colunas em 2 partes: `[(0,0,0), (0,1,1), (1,0,2)]`.
    fn two_part_mapping() -> CharacterMapping {
        let mut mapping = CharacterMapping::new(3, 2);
        mapping.set_column_info(1, 0, 0, 0);
        mapping.set_column_info(2, 0, 1, 1);
        mapping.set_column_info(3, 1, 0, 2);
        mapping
    }

    #[test]
    fn horizontal_offset_by_column() {
        let mapping = two_part_mapping();
        assert_eq!(mapping.get_horizontal_offset(1), 0);
        assert_eq!(mapping.get_horizontal_offset(2), 1);
        assert_eq!(mapping.get_horizontal_offset(3), 2);
    }

    #[test]
    fn dom_position_clamps_out_of_range_columns() {
        let mapping = two_part_mapping();
        assert_eq!(mapping.get_dom_position(2), DomPosition::new(0, 1));
        assert_eq!(mapping.get_dom_position(3), DomPosition::new(1, 0));
        // Antes do começo vale a primeira coluna, depois do fim vale a última.
        assert_eq!(mapping.get_dom_position(0), DomPosition::new(0, 0));
        assert_eq!(mapping.get_dom_position(10), DomPosition::new(1, 0));
    }

    #[test]
    fn inflate_lists_part_char_and_offset_per_column() {
        assert_eq!(two_part_mapping().inflate(), vec![(0, 0, 0), (0, 1, 1), (1, 0, 2)]);
    }

    #[test]
    fn get_column_finds_exact_entry() {
        let mapping = two_part_mapping();
        assert_eq!(mapping.get_column(&DomPosition::new(0, 1), 2), 2);
        assert_eq!(mapping.get_column(&DomPosition::new(1, 0), 1), 3);
    }

    #[test]
    fn get_column_picks_nearest_entry_inside_a_part() {
        let mut mapping = CharacterMapping::new(3, 2);
        mapping.set_column_info(1, 0, 0, 0);
        mapping.set_column_info(2, 0, 4, 1);
        mapping.set_column_info(3, 1, 0, 5);
        // Distância 2 para cada lado: empate fica com a primeira.
        assert_eq!(mapping.get_column(&DomPosition::new(0, 2), 6), 1);
        // Mais perto da segunda entrada.
        assert_eq!(mapping.get_column(&DomPosition::new(0, 3), 6), 2);
    }

    #[test]
    fn get_column_between_parts_uses_part_length() {
        let mut mapping = CharacterMapping::new(2, 2);
        mapping.set_column_info(1, 0, 0, 0);
        mapping.set_column_info(2, 1, 0, 6);
        // O fim da parte 0 tem comprimento 6: o caractere 5 está mais perto da coluna 2.
        assert_eq!(mapping.get_column(&DomPosition::new(0, 5), 6), 2);
        assert_eq!(mapping.get_column(&DomPosition::new(0, 1), 6), 1);
    }

    #[test]
    fn empty_mapping_answers_zero() {
        let mapping = CharacterMapping::new(0, 0);
        assert_eq!(mapping.get_horizontal_offset(1), 0);
        assert_eq!(mapping.get_dom_position(1), DomPosition::new(0, 0));
        assert_eq!(mapping.get_column(&DomPosition::new(0, 0), 0), 1);
        assert!(mapping.inflate().is_empty());
    }

    #[test]
    fn set_column_info_out_of_range_is_ignored() {
        let mut mapping = CharacterMapping::new(1, 1);
        mapping.set_column_info(0, 3, 3, 3);
        mapping.set_column_info(2, 3, 3, 3);
        assert_eq!(mapping.inflate(), vec![(0, 0, 0)]);
    }

    #[test]
    fn overflow_size_text_rounds_half_up_like_to_fixed() {
        assert_eq!(render_overflowing_char_count(500), "500 chars");
        assert_eq!(render_overflowing_char_count(1280), "1.3 KB");
        assert_eq!(render_overflowing_char_count(2048), "2.0 KB");
        assert_eq!(render_overflowing_char_count(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn js_number_to_string_matches_javascript_for_widths() {
        assert_eq!(js_number_to_string(8.0), "8");
        assert_eq!(js_number_to_string(7.2), "7.2");
        assert_eq!(js_number_to_string(0.0), "0");
    }
}
