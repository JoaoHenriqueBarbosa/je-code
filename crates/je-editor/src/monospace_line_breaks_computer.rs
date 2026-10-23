//! Porte de `src/vs/editor/common/viewModel/monospaceLineBreaksComputer.ts`.
//!
//! Decisões de tradução:
//! - O texto da linha é uma fatia de unidades UTF-16 (`&[u16]`), e um código de caractere é um `i32`: o
//!   `NaN` que `charCodeAt` devolve fora do texto vira [`NAN_CHAR_CODE`] (`-1`), que nenhuma comparação do
//!   algoritmo confunde com um caractere (`isFullWidthCharacter`, `isHighSurrogate`, a faixa de controle
//!   `0..32` e as faixas ideográficas o rejeitam, e o classificador devolve a classe padrão).
//! - Larguras, colunas visíveis e `tabSize` são `f64`: uma fonte que "mente" sobre a razão do caractere
//!   largo dá colunas fracionárias, e o `%` do JavaScript é o `%` de `f64`. Deslocamentos são `usize`; só os
//!   índices que o original deixa chegar a `-1` (`prevIndex`, o `i` da busca para a esquerda) são `i64`.
//! - As listas reaproveitadas `arrPool1` e `arrPool2` (otimização contra pausas do coletor de lixo) não
//!   existem: cada chamada preenche vetores novos, e o resultado é o mesmo porque o original só lê as
//!   posições que acabou de gravar.
//! - Os parâmetros que as duas funções de cálculo repetem (largura de caractere, coluna de quebra, recuo,
//!   `keepAll`) ficam em [`WrapSettings`].
//! - `CharacterClass` (`const enum`) vira um `enum`, e o classificador guarda o valor como `u8`.
//! - `createLineBreaksFromPreviousLineBreaks` recebe o dado anterior por valor e o devolve alterado, como o
//!   original faz com o mesmo objeto.

use je_base::strings;
use je_base::JsString;
use crate::character_classifier::CharacterClassifier;
use crate::config::editor_options::registry::{WORD_WRAP_BREAK_AFTER_CHARACTERS, WORD_WRAP_BREAK_BEFORE_CHARACTERS};
use crate::config::editor_options::wrapping_indent::WrappingIndent;
use crate::config::editor_options::ComputedEditorOptions;
use crate::config::font_info::FontInfo;
use crate::model::InjectedTextOptions;
use crate::model_line_projection_data::{
    LineBreaksComputer, LineBreaksComputerContext, LineBreaksComputerFactory, ModelLineProjectionData, WordBreak,
};
use crate::text_model_events::{FixedWidthInjectedTextRange, LineInjectedText};

/// O `NaN` de `charCodeAt` fora do texto.
const NAN_CHAR_CODE: i32 = -1;
/// `CharCode.Null`.
const NULL_CHAR_CODE: i32 = 0;
/// `CharCode.Tab`.
const TAB_CHAR_CODE: i32 = 9;
/// `CharCode.Space`.
const SPACE_CHAR_CODE: i32 = 32;
/// `\`.
const BACKSLASH: u16 = 0x5C;

/// `charCodeAt(index)`: `NaN` fora do texto.
fn char_code_at(units: &[u16], index: usize) -> i32 {
    units.get(index).map_or(NAN_CHAR_CODE, |&unit| i32::from(unit))
}

/// `strings.isHighSurrogate` sobre um código que pode ser `NaN`.
fn is_high_surrogate(char_code: i32) -> bool {
    u16::try_from(char_code).is_ok_and(strings::is_high_surrogate)
}

/// `strings.isLowSurrogate` sobre um código que pode ser `NaN`.
fn is_low_surrogate(char_code: i32) -> bool {
    u16::try_from(char_code).is_ok_and(strings::is_low_surrogate)
}

/// `strings.isFullWidthCharacter` sobre um código que pode ser `NaN`.
fn is_full_width_character(char_code: i32) -> bool {
    u16::try_from(char_code).is_ok_and(strings::is_full_width_character)
}

/// `array[index]` de um vetor de números, com o `undefined` virando `NaN` fora dos limites.
fn number_at(values: &[f64], index: i64) -> f64 {
    usize::try_from(index).ok().and_then(|i| values.get(i)).copied().unwrap_or(f64::NAN)
}

pub struct MonospaceLineBreaksComputerFactory {
    classifier: WrappingCharacterClassifier,
}

impl MonospaceLineBreaksComputerFactory {
    /// `create(options)`.
    pub fn create(options: &ComputedEditorOptions) -> Self {
        Self::new(
            options.get(WORD_WRAP_BREAK_BEFORE_CHARACTERS),
            options.get(WORD_WRAP_BREAK_AFTER_CHARACTERS),
        )
    }

    pub fn new(break_before_chars: &str, break_after_chars: &str) -> Self {
        Self { classifier: WrappingCharacterClassifier::new(break_before_chars, break_after_chars) }
    }
}

impl LineBreaksComputerFactory for MonospaceLineBreaksComputerFactory {
    #[allow(clippy::too_many_arguments)]
    fn create_line_breaks_computer<'a>(
        &'a self,
        context: &'a dyn LineBreaksComputerContext,
        font_info: &FontInfo,
        tab_size: f64,
        wrapping_column: f64,
        wrapping_indent: WrappingIndent,
        word_break: WordBreak,
        wrap_on_escaped_line_feeds: bool,
        use_two_cell_fullwidth_characters: bool,
    ) -> Box<dyn LineBreaksComputer + 'a> {
        Box::new(MonospaceLineBreaksComputer {
            classifier: &self.classifier,
            context,
            font_info: font_info.clone(),
            tab_size,
            wrapping_column,
            wrapping_indent,
            word_break,
            wrap_on_escaped_line_feeds,
            use_two_cell_fullwidth_characters,
            line_numbers: Vec::new(),
            previous_breaking_data: Vec::new(),
        })
    }
}

/// O objeto que `createLineBreaksComputer` devolve no original (`addRequest` e `finalize`).
struct MonospaceLineBreaksComputer<'a> {
    classifier: &'a WrappingCharacterClassifier,
    context: &'a dyn LineBreaksComputerContext,
    font_info: FontInfo,
    tab_size: f64,
    wrapping_column: f64,
    wrapping_indent: WrappingIndent,
    word_break: WordBreak,
    wrap_on_escaped_line_feeds: bool,
    use_two_cell_fullwidth_characters: bool,
    line_numbers: Vec<i32>,
    previous_breaking_data: Vec<Option<ModelLineProjectionData>>,
}

impl LineBreaksComputer for MonospaceLineBreaksComputer<'_> {
    fn add_request(&mut self, line_number: i32, previous_line_break_data: Option<ModelLineProjectionData>) {
        self.line_numbers.push(line_number);
        self.previous_breaking_data.push(previous_line_break_data);
    }

    fn finalize(&mut self) -> Vec<Option<ModelLineProjectionData>> {
        let columns_for_full_width_char =
            self.font_info.typical_fullwidth_character_width / self.font_info.typical_halfwidth_character_width;
        let settings = WrapSettings {
            metrics: CharMetrics {
                tab_size: self.tab_size,
                columns_for_full_width_char,
                use_two_cell_fullwidth_characters: self.use_two_cell_fullwidth_characters,
            },
            first_line_break_column: self.wrapping_column,
            wrapping_indent: self.wrapping_indent,
            is_keep_all: self.word_break == WordBreak::KeepAll,
            font_info: &self.font_info,
        };
        let line_numbers = std::mem::take(&mut self.line_numbers);
        let previous_breaking_data = std::mem::take(&mut self.previous_breaking_data);
        line_numbers
            .into_iter()
            .zip(previous_breaking_data)
            .map(|(line_number, previous_line_break_data)| {
                let injected_text = self.context.get_line_injected_text(line_number);
                let line_text = self.context.get_line_content(line_number);
                let units = line_text.units();
                let is_line_feed_wrapping_enabled = self.wrap_on_escaped_line_feeds
                    && units.contains(&u16::from(b'"'))
                    && units.windows(2).any(|pair| pair == [BACKSLASH, u16::from(b'n')]);
                match previous_line_break_data {
                    Some(previous)
                        if previous.injection_options.is_none()
                            && injected_text.is_none()
                            && !is_line_feed_wrapping_enabled =>
                    {
                        create_line_breaks_from_previous_line_breaks(self.classifier, previous, units, &settings)
                    }
                    _ => create_line_breaks(
                        self.classifier,
                        units,
                        injected_text.as_deref(),
                        &settings,
                        is_line_feed_wrapping_enabled,
                    ),
                }
            })
            .collect()
    }
}

/// `CharacterClass`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharacterClass {
    /// `NONE`.
    NoClass = 0,
    BreakBefore = 1,
    BreakAfter = 2,
    /// `BREAK_IDEOGRAPHIC`: para Han e Kana.
    BreakIdeographic = 3,
}

impl CharacterClass {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => CharacterClass::BreakBefore,
            2 => CharacterClass::BreakAfter,
            3 => CharacterClass::BreakIdeographic,
            _ => CharacterClass::NoClass,
        }
    }
}

struct WrappingCharacterClassifier {
    classifier: CharacterClassifier,
}

impl WrappingCharacterClassifier {
    fn new(break_before: &str, break_after: &str) -> Self {
        let mut classifier = CharacterClassifier::new(CharacterClass::NoClass as u8);

        for unit in break_before.encode_utf16() {
            classifier.set(i32::from(unit), CharacterClass::BreakBefore as u8);
        }

        for unit in break_after.encode_utf16() {
            classifier.set(i32::from(unit), CharacterClass::BreakAfter as u8);
        }

        Self { classifier }
    }

    fn get(&self, char_code: i32) -> CharacterClass {
        // Initialize CharacterClass.BREAK_IDEOGRAPHIC for these Unicode ranges:
        // 1. CJK Unified Ideographs (0x4E00 -- 0x9FFF)
        // 2. CJK Unified Ideographs Extension A (0x3400 -- 0x4DBF)
        // 3. Hiragana and Katakana (0x3040 -- 0x30FF)
        if !(0..256).contains(&char_code)
            && ((0x3040..=0x30FF).contains(&char_code)
                || (0x3400..=0x4DBF).contains(&char_code)
                || (0x4E00..=0x9FFF).contains(&char_code))
        {
            return CharacterClass::BreakIdeographic;
        }
        CharacterClass::from_u8(self.classifier.get(char_code))
    }
}

/// As medidas em colunas de um caractere: `computeCharWidth`, `fullWidthCharacterColumns` e
/// `tabCharacterWidth`.
#[derive(Clone, Copy)]
struct CharMetrics {
    tab_size: f64,
    columns_for_full_width_char: f64,
    use_two_cell_fullwidth_characters: bool,
}

impl CharMetrics {
    /// `fullWidthCharacterColumns`: exatamente duas células quando os caracteres largos são forçados a duas
    /// células monoespaçadas, senão a razão medida da fonte.
    fn full_width_columns(self) -> f64 {
        if self.use_two_cell_fullwidth_characters { 2.0 } else { self.columns_for_full_width_char }
    }

    /// `tabCharacterWidth`.
    fn tab_width(self, visible_column: f64) -> f64 {
        self.tab_size - (visible_column % self.tab_size)
    }

    /// `computeCharWidth`.
    fn char_width(self, char_code: i32, visible_column: f64) -> f64 {
        if char_code == TAB_CHAR_CODE {
            return self.tab_width(visible_column);
        }
        if is_full_width_character(char_code) {
            return self.full_width_columns();
        }
        if (0..32).contains(&char_code) {
            // when using `editor.renderControlCharacters`, the substitutions are often wide
            return self.columns_for_full_width_char;
        }
        1.0
    }

    /// `computeCharPixelWidth`: a largura em pixels de um caractere, usada na decisão de quebra, que precisa
    /// raciocinar em larguras reais porque o texto injetado pode pedir uma largura arbitrária via `widthInEm`.
    fn char_pixel_width(self, char_code: i32, visible_column: f64, font_info: &FontInfo) -> f64 {
        if char_code == TAB_CHAR_CODE {
            return self.tab_width(visible_column) * font_info.typical_halfwidth_character_width;
        }
        if is_full_width_character(char_code) {
            return if self.use_two_cell_fullwidth_characters {
                2.0 * font_info.typical_halfwidth_character_width
            } else {
                font_info.typical_fullwidth_character_width
            };
        }
        if (0..32).contains(&char_code) {
            // when using `editor.renderControlCharacters`, the substitutions are often wide
            return font_info.typical_fullwidth_character_width;
        }
        font_info.typical_halfwidth_character_width
    }
}

/// Os parâmetros que o cálculo inteiro e o incremental compartilham.
struct WrapSettings<'a> {
    metrics: CharMetrics,
    first_line_break_column: f64,
    wrapping_indent: WrappingIndent,
    is_keep_all: bool,
    font_info: &'a FontInfo,
}

fn create_line_breaks_from_previous_line_breaks(
    classifier: &WrappingCharacterClassifier,
    mut previous_breaking_data: ModelLineProjectionData,
    line_text: &[u16],
    settings: &WrapSettings,
) -> Option<ModelLineProjectionData> {
    let first_line_break_column = settings.first_line_break_column;
    if first_line_break_column == -1.0 {
        return None;
    }

    let len = line_text.len();
    if len <= 1 {
        return None;
    }

    let metrics = settings.metrics;
    let tab_size = metrics.tab_size;
    let is_keep_all = settings.is_keep_all;

    let prev_breaking_offsets = &previous_breaking_data.break_offsets;
    let prev_breaking_offsets_visible_column = &previous_breaking_data.break_offsets_visible_column;

    let wrapped_text_indent_length = compute_wrapped_text_indent_length(line_text, settings, &[]);
    let wrapped_line_break_column = first_line_break_column - wrapped_text_indent_length as f64;

    let mut breaking_offsets: Vec<usize> = Vec::new();
    let mut breaking_offsets_visible_column: Vec<f64> = Vec::new();
    let mut last_breaking_offset = 0usize;
    let mut last_breaking_offset_visible_column = 0.0f64;

    let mut breaking_column = first_line_break_column;
    let prev_len = prev_breaking_offsets.len() as i64;
    let mut prev_index: i64 = 0;

    // `if (prevIndex >= 0)` do original vale sempre aqui: o índice acabou de ser zerado.
    let mut best_distance = (number_at(prev_breaking_offsets_visible_column, prev_index) - breaking_column).abs();
    while prev_index + 1 < prev_len {
        let distance = (number_at(prev_breaking_offsets_visible_column, prev_index + 1) - breaking_column).abs();
        if distance >= best_distance {
            break;
        }
        best_distance = distance;
        prev_index += 1;
    }

    while prev_index < prev_len {
        // Allow for prevIndex to be -1 (for the case where we hit a tab when walking backwards from the first break)
        let (mut prev_break_offset, mut prev_break_offset_visible_column) = if prev_index < 0 {
            (0, 0.0)
        } else {
            (
                prev_breaking_offsets[prev_index as usize],
                number_at(prev_breaking_offsets_visible_column, prev_index),
            )
        };
        if last_breaking_offset > prev_break_offset {
            prev_break_offset = last_breaking_offset;
            prev_break_offset_visible_column = last_breaking_offset_visible_column;
        }

        let mut break_offset = 0usize;
        let mut break_offset_visible_column = 0.0f64;

        let mut forced_break_offset = 0usize;
        let mut forced_break_offset_visible_column = 0.0f64;

        // initially, we search as much as possible to the right (if it fits)
        if prev_break_offset_visible_column <= breaking_column {
            let mut visible_column = prev_break_offset_visible_column;
            // `prevCharCode` do original só alimenta `canBreak`, que não o lê: fica apenas a classe.
            let mut prev_char_code_class = if prev_break_offset == 0 {
                CharacterClass::NoClass
            } else {
                classifier.get(char_code_at(line_text, prev_break_offset - 1))
            };
            let mut entire_line_fits = true;
            let mut i = prev_break_offset;
            while i < len {
                let char_start_offset = i;
                let char_code = char_code_at(line_text, i);
                let char_code_class;
                let char_width;

                if is_high_surrogate(char_code) {
                    // A surrogate pair must always be considered as a single unit, so it is never to be broken
                    i += 1;
                    char_code_class = CharacterClass::NoClass;
                    char_width = 2.0;
                } else {
                    char_code_class = classifier.get(char_code);
                    char_width = metrics.char_width(char_code, visible_column);
                }

                if char_start_offset > last_breaking_offset
                    && can_break(prev_char_code_class, char_code, char_code_class, is_keep_all)
                {
                    break_offset = char_start_offset;
                    break_offset_visible_column = visible_column;
                }

                visible_column += char_width;

                // check if adding character at `i` will go over the breaking column
                if visible_column > breaking_column {
                    // We need to break at least before character at `i`:
                    if char_start_offset > last_breaking_offset {
                        forced_break_offset = char_start_offset;
                        forced_break_offset_visible_column = visible_column - char_width;
                    } else {
                        // we need to advance at least by one character
                        forced_break_offset = i + 1;
                        forced_break_offset_visible_column = visible_column;
                    }

                    if visible_column - break_offset_visible_column > wrapped_line_break_column {
                        // Cannot break at `breakOffset` => reset it if it was set
                        break_offset = 0;
                    }

                    entire_line_fits = false;
                    break;
                }

                prev_char_code_class = char_code_class;
                i += 1;
            }

            if entire_line_fits {
                // there is no more need to break => stop the outer loop!
                if !breaking_offsets.is_empty() {
                    // Add last segment, no need to assign to `lastBreakingOffset` and `lastBreakingOffsetVisibleColumn`
                    breaking_offsets.push(prev_breaking_offsets[prev_breaking_offsets.len() - 1]);
                    breaking_offsets_visible_column.push(number_at(
                        prev_breaking_offsets_visible_column,
                        prev_breaking_offsets.len() as i64 - 1,
                    ));
                }
                break;
            }
        }

        if break_offset == 0 {
            // must search left
            let mut visible_column = prev_break_offset_visible_column;
            let mut char_code = char_code_at(line_text, prev_break_offset);
            let mut char_code_class = classifier.get(char_code);
            let mut hit_a_tab_character = false;
            let mut i = prev_break_offset as i64 - 1;
            while i >= last_breaking_offset as i64 {
                let char_start_offset = (i + 1) as usize;
                let prev_char_code = char_code_at(line_text, i as usize);

                if prev_char_code == TAB_CHAR_CODE {
                    // cannot determine the width of a tab when going backwards, so we must go forwards
                    hit_a_tab_character = true;
                    break;
                }

                let prev_char_code_class;
                let prev_char_width;

                if is_low_surrogate(prev_char_code) {
                    // A surrogate pair must always be considered as a single unit, so it is never to be broken
                    i -= 1;
                    prev_char_code_class = CharacterClass::NoClass;
                    prev_char_width = 2.0;
                } else {
                    prev_char_code_class = classifier.get(prev_char_code);
                    prev_char_width = if is_full_width_character(prev_char_code) { metrics.full_width_columns() } else { 1.0 };
                }

                if visible_column <= breaking_column {
                    if forced_break_offset == 0 {
                        forced_break_offset = char_start_offset;
                        forced_break_offset_visible_column = visible_column;
                    }

                    if visible_column <= breaking_column - wrapped_line_break_column {
                        // went too far!
                        break;
                    }

                    if can_break(prev_char_code_class, char_code, char_code_class, is_keep_all) {
                        break_offset = char_start_offset;
                        break_offset_visible_column = visible_column;
                        break;
                    }
                }

                visible_column -= prev_char_width;
                char_code = prev_char_code;
                char_code_class = prev_char_code_class;
                i -= 1;
            }

            if break_offset != 0 {
                let remaining_width_of_next_line =
                    wrapped_line_break_column - (forced_break_offset_visible_column - break_offset_visible_column);
                if remaining_width_of_next_line <= tab_size {
                    let char_code_at_forced_break_offset = char_code_at(line_text, forced_break_offset);
                    let char_width = if is_high_surrogate(char_code_at_forced_break_offset) {
                        // A surrogate pair must always be considered as a single unit, so it is never to be broken
                        2.0
                    } else {
                        metrics.char_width(char_code_at_forced_break_offset, forced_break_offset_visible_column)
                    };
                    if remaining_width_of_next_line - char_width < 0.0 {
                        // it is not worth it to break at breakOffset, it just introduces an extra needless line!
                        break_offset = 0;
                    }
                }
            }

            if hit_a_tab_character {
                // cannot determine the width of a tab when going backwards, so we must go forwards from the previous break
                prev_index -= 1;
                continue;
            }
        }

        if break_offset == 0 {
            // Could not find a good breaking point
            break_offset = forced_break_offset;
            break_offset_visible_column = forced_break_offset_visible_column;
        }

        if break_offset <= last_breaking_offset {
            // Make sure that we are advancing (at least one character)
            let char_code = char_code_at(line_text, last_breaking_offset);
            if is_high_surrogate(char_code) {
                // A surrogate pair must always be considered as a single unit, so it is never to be broken
                break_offset = last_breaking_offset + 2;
                break_offset_visible_column = last_breaking_offset_visible_column + 2.0;
            } else {
                break_offset = last_breaking_offset + 1;
                break_offset_visible_column =
                    last_breaking_offset_visible_column + metrics.char_width(char_code, last_breaking_offset_visible_column);
            }
        }

        last_breaking_offset = break_offset;
        breaking_offsets.push(break_offset);
        last_breaking_offset_visible_column = break_offset_visible_column;
        breaking_offsets_visible_column.push(break_offset_visible_column);
        breaking_column = break_offset_visible_column + wrapped_line_break_column;

        while prev_index < 0
            || (prev_index < prev_len
                && number_at(prev_breaking_offsets_visible_column, prev_index) < break_offset_visible_column)
        {
            prev_index += 1;
        }

        let mut best_distance = (number_at(prev_breaking_offsets_visible_column, prev_index) - breaking_column).abs();
        while prev_index + 1 < prev_len {
            let distance = (number_at(prev_breaking_offsets_visible_column, prev_index + 1) - breaking_column).abs();
            if distance >= best_distance {
                break;
            }
            best_distance = distance;
            prev_index += 1;
        }
    }

    if breaking_offsets.is_empty() {
        return None;
    }

    previous_breaking_data.break_offsets = breaking_offsets;
    previous_breaking_data.break_offsets_visible_column = breaking_offsets_visible_column;
    previous_breaking_data.wrapped_text_indent_length = wrapped_text_indent_length;
    Some(previous_breaking_data)
}

fn create_line_breaks(
    classifier: &WrappingCharacterClassifier,
    line_text_without_injections: &[u16],
    injected_texts: Option<&[LineInjectedText]>,
    settings: &WrapSettings,
    wrap_on_escaped_line_feeds: bool,
) -> Option<ModelLineProjectionData> {
    let line_text_with_injections =
        LineInjectedText::apply_injected_text(&JsString::from(line_text_without_injections), injected_texts);
    let line_text = line_text_with_injections.units();
    let fixed_width_ranges = LineInjectedText::get_fixed_width_injected_text_ranges(injected_texts);

    let (injection_options, injection_offsets): (Option<Vec<InjectedTextOptions>>, Option<Vec<usize>>) =
        match injected_texts.filter(|texts| !texts.is_empty()) {
            Some(texts) => (
                Some(texts.iter().map(|text| text.options.clone()).collect()),
                Some(texts.iter().map(|text| (text.column - 1).max(0) as usize).collect()),
            ),
            None => (None, None),
        };

    let first_line_break_column = settings.first_line_break_column;
    if first_line_break_column == -1.0 {
        injection_options.as_ref()?;
        // creating a `LineBreakData` with an invalid `breakOffsetsVisibleColumn` is OK
        // because `breakOffsetsVisibleColumn` will never be used because it contains injected text
        return Some(ModelLineProjectionData::new(
            injection_offsets,
            injection_options,
            vec![line_text.len()],
            Vec::new(),
            0,
        ));
    }

    let len = line_text.len();
    if len <= 1 {
        injection_options.as_ref()?;
        // creating a `LineBreakData` with an invalid `breakOffsetsVisibleColumn` is OK
        // because `breakOffsetsVisibleColumn` will never be used because it contains injected text
        return Some(ModelLineProjectionData::new(injection_offsets, injection_options, vec![len], Vec::new(), 0));
    }

    let metrics = settings.metrics;
    let font_info = settings.font_info;
    let is_keep_all = settings.is_keep_all;
    let wrapped_text_indent_length = compute_wrapped_text_indent_length(line_text, settings, &fixed_width_ranges);

    // The wrapping decision is taken in pixels, because injected text can request an arbitrary
    // width via `widthInEm` which does not map to a whole number of columns.
    let typical_halfwidth_character_width = font_info.typical_halfwidth_character_width;
    let wrapped_line_break_pixel_width =
        (first_line_break_column - wrapped_text_indent_length as f64) * typical_halfwidth_character_width;

    let mut breaking_offsets: Vec<usize> = Vec::new();
    let mut breaking_offsets_visible_column: Vec<f64> = Vec::new();
    let mut break_offset = 0usize;
    let mut break_offset_visible_column = 0.0f64;
    let mut break_offset_pixel_width = 0.0f64;

    let mut breaking_pixel_width = first_line_break_column * typical_halfwidth_character_width;
    let mut fixed_width_range_index = 0usize;
    let first_fixed_width_range = fixed_width_ranges.first().copied().filter(|range| range.start_offset == 0);

    let mut prev_char_code_class: CharacterClass;
    let mut visible_column: f64;
    let mut current_line_pixel_width: f64;
    let start_offset: usize;
    if let Some(range) = first_fixed_width_range {
        prev_char_code_class = CharacterClass::NoClass;
        visible_column = compute_fixed_width_range_column_width(line_text, &range, 0.0, metrics);
        current_line_pixel_width = range.width_in_em * font_info.font_size;
        start_offset = range.end_offset;
        fixed_width_range_index += 1;
    } else {
        let mut offset = 1usize;
        let first_char_code = char_code_at(line_text, 0);
        // O caractere cuja classe vira `prevCharCodeClass` (o do par, se o primeiro é um surrogate alto).
        let mut prev_char_code = first_char_code;
        visible_column = metrics.char_width(first_char_code, 0.0);
        current_line_pixel_width = metrics.char_pixel_width(first_char_code, 0.0, font_info);
        if is_high_surrogate(first_char_code) {
            // A surrogate pair must always be considered as a single unit, so it is never to be broken
            visible_column += 1.0;
            current_line_pixel_width += typical_halfwidth_character_width;
            prev_char_code = char_code_at(line_text, 1);
            offset += 1;
        }
        prev_char_code_class = classifier.get(prev_char_code);
        start_offset = offset;
    }

    let mut i = start_offset;
    while i < len {
        let fixed_width_range = fixed_width_ranges.get(fixed_width_range_index).copied();
        let char_start_offset = i;
        let mut char_code = char_code_at(line_text, i);
        let char_code_class: CharacterClass;
        let char_width: f64;
        let char_pixel_width: f64;
        let mut wrap_escaped_line_feed = false;

        if let Some(range) = fixed_width_range.filter(|range| range.start_offset == i) {
            char_code = NULL_CHAR_CODE;
            char_code_class = CharacterClass::NoClass;
            char_width = compute_fixed_width_range_column_width(line_text, &range, visible_column, metrics);
            char_pixel_width = range.width_in_em * font_info.font_size;
            // `endOffset - 1` do original: o laço soma 1 logo em seguida, e uma faixa vazia na posição 0
            // (que o original deixa passar por -1) volta ao mesmo lugar em vez de estourar.
            i = range.end_offset.wrapping_sub(1);
            fixed_width_range_index += 1;
        } else if is_high_surrogate(char_code) {
            // A surrogate pair must always be considered as a single unit, so it is never to be broken
            i += 1;
            char_code_class = CharacterClass::NoClass;
            char_width = 2.0;
            char_pixel_width = 2.0 * typical_halfwidth_character_width;
        } else {
            char_code_class = classifier.get(char_code);
            char_width = metrics.char_width(char_code, visible_column);
            char_pixel_width = metrics.char_pixel_width(char_code, visible_column, font_info);
        }

        // literal \n shall trigger a softwrap
        if wrap_on_escaped_line_feeds && is_escaped_line_break_at_position(line_text, char_start_offset) {
            break_offset = char_start_offset;
            break_offset_visible_column = visible_column;
            break_offset_pixel_width = current_line_pixel_width;
            wrap_escaped_line_feed = true;
        } else if can_break(prev_char_code_class, char_code, char_code_class, is_keep_all) {
            break_offset = char_start_offset;
            break_offset_visible_column = visible_column;
            break_offset_pixel_width = current_line_pixel_width;
        }

        visible_column += char_width;
        current_line_pixel_width += char_pixel_width;

        // check if adding character at `i` will go over the breaking width
        if current_line_pixel_width > breaking_pixel_width || wrap_escaped_line_feed {
            // We need to break at least before character at `i`:

            if break_offset == 0 || current_line_pixel_width - break_offset_pixel_width > wrapped_line_break_pixel_width {
                // Cannot break at `breakOffset`, must break at `i`
                break_offset = char_start_offset;
                break_offset_visible_column = visible_column - char_width;
                break_offset_pixel_width = current_line_pixel_width - char_pixel_width;
            }

            let current_line_start_offset = breaking_offsets.last().copied().unwrap_or(0);
            if break_offset > current_line_start_offset {
                // Breaking at the start of the current output line would emit an empty line, which
                // happens when an oversized leading width-only injection cannot fit before the first
                // character. In that case keep the content on the current line and defer the break.
                breaking_offsets.push(break_offset);
                breaking_offsets_visible_column.push(break_offset_visible_column);
                breaking_pixel_width = break_offset_pixel_width + wrapped_line_break_pixel_width;
                break_offset = 0;
            }
        }

        prev_char_code_class = char_code_class;
        i = i.wrapping_add(1);
    }

    if breaking_offsets.is_empty() && injected_texts.is_none_or(|texts| texts.is_empty()) {
        return None;
    }

    // Add last segment
    breaking_offsets.push(len);
    breaking_offsets_visible_column.push(visible_column);

    Some(ModelLineProjectionData::new(
        injection_offsets,
        injection_options,
        breaking_offsets,
        breaking_offsets_visible_column,
        wrapped_text_indent_length,
    ))
}

/// `computeFixedWidthRangeColumnWidth`: o número de colunas que os caracteres de uma faixa de largura fixa
/// ocupam, que é 0 para um texto injetado só de largura. `widthInEm` é ignorado de propósito: `visibleColumn`
/// só dirige a expansão de tabs, que precisa ficar em sincronia com o desenho da linha, e o desenho não conhece
/// `widthInEm`.
fn compute_fixed_width_range_column_width(
    line_text: &[u16],
    range: &FixedWidthInjectedTextRange,
    visible_column: f64,
    metrics: CharMetrics,
) -> f64 {
    let mut width = 0.0;
    for i in range.start_offset..range.end_offset {
        width += metrics.char_width(char_code_at(line_text, i), visible_column + width);
    }
    width
}

/// Checks if the current position in the text should trigger a soft wrap due to escaped line feeds.
/// This handles the wrapOnEscapedLineFeeds feature which allows \n sequences in strings to trigger wrapping.
fn is_escaped_line_break_at_position(line_text: &[u16], i: usize) -> bool {
    if i >= 2 && line_text.get(i - 1) == Some(&u16::from(b'n')) {
        // Check if there's an odd number of backslashes
        let mut escape_count = 0;
        for j in (0..=i - 2).rev() {
            if line_text.get(j) == Some(&BACKSLASH) {
                escape_count += 1;
            } else {
                return escape_count % 2 == 1;
            }
        }
    }
    false
}

/// Kinsoku Shori : Don't break after a leading character, like an open bracket
/// Kinsoku Shori : Don't break before a trailing character, like a period
///
/// O `prevCharCode` do original não é lido pela condição, então só a classe do anterior chega aqui.
fn can_break(
    prev_char_code_class: CharacterClass,
    char_code: i32,
    char_code_class: CharacterClass,
    is_keep_all: bool,
) -> bool {
    char_code != SPACE_CHAR_CODE
        && (
            // break at the end of multiple BREAK_AFTER
            (prev_char_code_class == CharacterClass::BreakAfter && char_code_class != CharacterClass::BreakAfter)
            // break at the start of multiple BREAK_BEFORE
            || (prev_char_code_class != CharacterClass::BreakBefore && char_code_class == CharacterClass::BreakBefore)
            || (!is_keep_all
                && prev_char_code_class == CharacterClass::BreakIdeographic
                && char_code_class != CharacterClass::BreakAfter)
            || (!is_keep_all
                && char_code_class == CharacterClass::BreakIdeographic
                && prev_char_code_class != CharacterClass::BreakBefore)
        )
}

/// `computeWrappedTextIndentLength`. `fixed_width_ranges` vazio é o `undefined` do original.
fn compute_wrapped_text_indent_length(
    line_text: &[u16],
    settings: &WrapSettings,
    fixed_width_ranges: &[FixedWidthInjectedTextRange],
) -> usize {
    let metrics = settings.metrics;
    let mut wrapped_text_indent_length = 0.0f64;
    if settings.wrapping_indent != WrappingIndent::None {
        if let Some(first_non_whitespace_index) = strings::first_non_whitespace_index(line_text) {
            // Track existing indent

            for (i, &unit) in line_text.iter().enumerate().take(first_non_whitespace_index) {
                if fixed_width_ranges.first().is_some_and(|range| range.start_offset == i) {
                    break;
                }
                let char_width = if i32::from(unit) == TAB_CHAR_CODE { metrics.tab_width(wrapped_text_indent_length) } else { 1.0 };
                wrapped_text_indent_length += char_width;
            }

            // Increase indent of continuation lines, if desired
            let number_of_additional_tabs = match settings.wrapping_indent {
                WrappingIndent::DeepIndent => 2,
                WrappingIndent::Indent => 1,
                _ => 0,
            };
            for _ in 0..number_of_additional_tabs {
                wrapped_text_indent_length += metrics.tab_width(wrapped_text_indent_length);
            }

            // Force sticking to beginning of line if no character would fit except for the indentation
            if wrapped_text_indent_length + metrics.full_width_columns() > settings.first_line_break_column {
                wrapped_text_indent_length = 0.0;
            }
        }
    }
    wrapped_text_indent_length as usize
}
