//! Porte de `src/vs/editor/common/modelLineProjectionData.ts`.
//!
//! Decisões de tradução:
//! - Deslocamentos, índices de linha de saída e comprimentos são `usize` (no original são `number`, sempre
//!   inteiros e não negativos); `Math.max(0, a - b)` vira `saturating_sub`. As colunas visíveis
//!   (`breakOffsetsVisibleColumn`) ficam `f64` porque a largura de um caractere largo pode ser fracionária.
//! - O comprimento do texto injetado é o do `string` do JavaScript, em unidades UTF-16
//!   (`content.encode_utf16().count()`).
//! - `InjectedTextOptions.cursorStops` ausente (`null | undefined`) é `Option::None`; `hasRightCursorStop` e
//!   `hasLeftCursorStop` tratam a ausência como "tem a parada".
//! - `assertNever(affinity)` some: o `match` sobre [`PositionAffinity`] já é exaustivo.
//! - `wordBreak: 'normal' | 'keepAll'` vira [`WordBreak`].
//! - `ILineBreaksComputerFactory` e `ILineBreaksComputer` viram traits. `addRequest` recebe o dado anterior
//!   por valor (o original o reaproveita e o devolve alterado) e `finalize` esvazia o computador.
//! - `ILineBreaksComputerContext` devolve o texto da linha como [`JsString`] (UTF-16) e os textos injetados
//!   como `Option<Vec<LineInjectedText>>` (`null` é `None`; uma lista vazia continua sendo "tem lista").

use std::fmt;

use je_base::JsString;

use crate::config::editor_options::wrapping_indent::WrappingIndent;
use crate::config::font_info::FontInfo;
use crate::model::{InjectedTextCursorStops, InjectedTextOptions, PositionAffinity};
use crate::position::Position;
use crate::text_model_events::LineInjectedText;

/// O comprimento (em unidades UTF-16) do conteúdo de um texto injetado: o `content.length` do original.
fn injected_length(options: &InjectedTextOptions) -> usize {
    options.content.encode_utf16().count()
}

/// *input*:
/// ```text
/// xxxxxxxxxxxxxxxxxxxxxxxxxxx
/// ```
///
/// -> Applying injections `[i...i]`, *inputWithInjections*:
/// ```text
/// xxxxxx[iiiiiiiiii]xxxxxxxxxxxxxxxxx[ii]xxxx
/// ```
///
/// -> breaking at offsets `|` in `xxxxxx[iiiiiii|iii]xxxxxxxxxxx|xxxxxx[ii]xxxx|`:
/// ```text
/// xxxxxx[iiiiiii
/// iii]xxxxxxxxxxx
/// xxxxxx[ii]xxxx
/// ```
///
/// -> applying wrappedTextIndentLength, *output*:
/// ```text
/// xxxxxx[iiiiiii
///    iii]xxxxxxxxxxx
///    xxxxxx[ii]xxxx
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ModelLineProjectionData {
    pub injection_offsets: Option<Vec<usize>>,
    /// `injection_options.len()` precisa ser igual a `injection_offsets.len()`.
    pub injection_options: Option<Vec<InjectedTextOptions>>,
    /// Refers to offsets after applying injections to the source.
    /// The last break offset indicates the length of the source after applying injections.
    pub break_offsets: Vec<usize>,
    /// Refers to offsets after applying injections.
    pub break_offsets_visible_column: Vec<f64>,
    pub wrapped_text_indent_length: usize,
}

/// O texto injetado que cobre um deslocamento (`getInjectedTextAtOffset`).
struct InjectedTextAtOffset {
    injected_text_index: usize,
    offset_in_input_with_injections: usize,
    length: usize,
}

impl ModelLineProjectionData {
    pub fn new(
        injection_offsets: Option<Vec<usize>>,
        injection_options: Option<Vec<InjectedTextOptions>>,
        break_offsets: Vec<usize>,
        break_offsets_visible_column: Vec<f64>,
        wrapped_text_indent_length: usize,
    ) -> Self {
        Self {
            injection_offsets,
            injection_options,
            break_offsets,
            break_offsets_visible_column,
            wrapped_text_indent_length,
        }
    }

    /// Os deslocamentos e as opções das injeções, quando há injeções (`injectionOffsets !== null`).
    fn injections(&self) -> Option<(&[usize], &[InjectedTextOptions])> {
        match (&self.injection_offsets, &self.injection_options) {
            (Some(offsets), Some(options)) => Some((offsets, options)),
            _ => None,
        }
    }

    pub fn get_output_line_count(&self) -> usize {
        self.break_offsets.len()
    }

    pub fn get_min_output_offset(&self, output_line_index: usize) -> usize {
        if output_line_index > 0 {
            return self.wrapped_text_indent_length;
        }
        0
    }

    pub fn get_line_length(&self, output_line_index: usize) -> usize {
        // These offsets refer to model text with injected text.
        let start_offset = if output_line_index > 0 { self.break_offsets[output_line_index - 1] } else { 0 };
        let end_offset = self.break_offsets[output_line_index];

        let mut line_length = end_offset - start_offset;
        if output_line_index > 0 {
            line_length += self.wrapped_text_indent_length;
        }
        line_length
    }

    pub fn get_max_output_offset(&self, output_line_index: usize) -> usize {
        self.get_line_length(output_line_index)
    }

    pub fn translate_to_input_offset(&self, output_line_index: usize, output_offset: usize) -> usize {
        let output_offset = if output_line_index > 0 {
            output_offset.saturating_sub(self.wrapped_text_indent_length)
        } else {
            output_offset
        };

        let offset_in_input_with_injection =
            if output_line_index == 0 { output_offset } else { self.break_offsets[output_line_index - 1] + output_offset };
        let mut offset_in_input = offset_in_input_with_injection;

        if let Some((injection_offsets, injection_options)) = self.injections() {
            for (injection_offset, options) in injection_offsets.iter().zip(injection_options) {
                if offset_in_input > *injection_offset {
                    let length = injected_length(options);
                    if offset_in_input < injection_offset + length {
                        // `inputOffset` is within injected text
                        offset_in_input = *injection_offset;
                    } else {
                        offset_in_input -= length;
                    }
                } else {
                    break;
                }
            }
        }

        offset_in_input
    }

    pub fn translate_to_output_position(&self, input_offset: usize, affinity: PositionAffinity) -> OutputPosition {
        let mut input_offset_in_input_with_injection = input_offset;
        if let Some((injection_offsets, injection_options)) = self.injections() {
            for (injection_offset, options) in injection_offsets.iter().zip(injection_options) {
                if input_offset < *injection_offset {
                    break;
                }

                if affinity != PositionAffinity::Right && input_offset == *injection_offset {
                    break;
                }

                input_offset_in_input_with_injection += injected_length(options);
            }
        }

        self.offset_in_input_with_injections_to_output_position(input_offset_in_input_with_injection, affinity)
    }

    fn offset_in_input_with_injections_to_output_position(
        &self,
        offset_in_input_with_injections: usize,
        affinity: PositionAffinity,
    ) -> OutputPosition {
        let mut low: i64 = 0;
        let mut high: i64 = self.break_offsets.len() as i64 - 1;
        let mut mid: i64 = 0;
        let mut mid_start = 0usize;

        while low <= high {
            mid = low + (high - low) / 2;

            let mid_stop = self.break_offsets[mid as usize];
            mid_start = if mid > 0 { self.break_offsets[mid as usize - 1] } else { 0 };

            if affinity == PositionAffinity::Left {
                if offset_in_input_with_injections <= mid_start {
                    high = mid - 1;
                } else if offset_in_input_with_injections > mid_stop {
                    low = mid + 1;
                } else {
                    break;
                }
            } else if offset_in_input_with_injections < mid_start {
                high = mid - 1;
            } else if offset_in_input_with_injections >= mid_stop {
                low = mid + 1;
            } else {
                break;
            }
        }

        let mut output_offset = offset_in_input_with_injections.wrapping_sub(mid_start);
        if mid > 0 {
            output_offset = output_offset.wrapping_add(self.wrapped_text_indent_length);
        }

        OutputPosition::new(mid as usize, output_offset)
    }

    pub fn normalize_output_position(
        &self,
        output_line_index: usize,
        output_offset: usize,
        affinity: PositionAffinity,
    ) -> OutputPosition {
        if self.injections().is_some() {
            let offset_in_input_with_injections =
                self.output_position_to_offset_in_input_with_injections(output_line_index, output_offset);
            let normalized_offset_in_unwrapped_line =
                self.normalize_offset_in_input_with_injections_around_injections(offset_in_input_with_injections, affinity);
            if normalized_offset_in_unwrapped_line != offset_in_input_with_injections {
                // injected text caused a change
                return self.offset_in_input_with_injections_to_output_position(normalized_offset_in_unwrapped_line, affinity);
            }
        }

        if affinity == PositionAffinity::Left {
            if output_line_index > 0 && output_offset == self.get_min_output_offset(output_line_index) {
                return OutputPosition::new(output_line_index - 1, self.get_max_output_offset(output_line_index - 1));
            }
        } else if affinity == PositionAffinity::Right {
            let max_output_line_index = self.get_output_line_count() - 1;
            if output_line_index < max_output_line_index && output_offset == self.get_max_output_offset(output_line_index) {
                return OutputPosition::new(output_line_index + 1, self.get_min_output_offset(output_line_index + 1));
            }
        }

        OutputPosition::new(output_line_index, output_offset)
    }

    fn output_position_to_offset_in_input_with_injections(&self, output_line_index: usize, output_offset: usize) -> usize {
        let output_offset =
            if output_line_index > 0 { output_offset.saturating_sub(self.wrapped_text_indent_length) } else { output_offset };
        (if output_line_index > 0 { self.break_offsets[output_line_index - 1] } else { 0 }) + output_offset
    }

    fn normalize_offset_in_input_with_injections_around_injections(
        &self,
        offset_in_input_with_injections: usize,
        affinity: PositionAffinity,
    ) -> usize {
        let Some(injected_text) = self.get_injected_text_at_offset(offset_in_input_with_injections) else {
            return offset_in_input_with_injections;
        };
        // `get_injected_text_at_offset` só acha texto quando há injeções.
        let Some((injection_offsets, injection_options)) = self.injections() else {
            return offset_in_input_with_injections;
        };

        match affinity {
            PositionAffinity::None => {
                if offset_in_input_with_injections == injected_text.offset_in_input_with_injections + injected_text.length
                    && has_right_cursor_stop(injection_options[injected_text.injected_text_index].cursor_stops)
                {
                    injected_text.offset_in_input_with_injections + injected_text.length
                } else {
                    let mut result = injected_text.offset_in_input_with_injections;
                    if has_left_cursor_stop(injection_options[injected_text.injected_text_index].cursor_stops) {
                        return result;
                    }

                    let mut index = injected_text.injected_text_index as i64 - 1;
                    while index >= 0
                        && injection_offsets[index as usize] == injection_offsets[injected_text.injected_text_index]
                    {
                        let options = &injection_options[index as usize];
                        if has_right_cursor_stop(options.cursor_stops) {
                            break;
                        }
                        result -= injected_length(options);
                        if has_left_cursor_stop(options.cursor_stops) {
                            break;
                        }
                        index -= 1;
                    }

                    result
                }
            }
            PositionAffinity::Right | PositionAffinity::RightOfInjectedText => {
                let mut result = injected_text.offset_in_input_with_injections + injected_text.length;
                let mut index = injected_text.injected_text_index;
                // traverse all injected text that touch each other
                while index + 1 < injection_offsets.len() && injection_offsets[index + 1] == injection_offsets[index] {
                    result += injected_length(&injection_options[index + 1]);
                    index += 1;
                }
                result
            }
            PositionAffinity::Left | PositionAffinity::LeftOfInjectedText => {
                // affinity is left
                let mut result = injected_text.offset_in_input_with_injections;
                let mut index = injected_text.injected_text_index;
                // traverse all injected text that touch each other
                while index >= 1 && injection_offsets[index - 1] == injection_offsets[index] {
                    result -= injected_length(&injection_options[index - 1]);
                    index -= 1;
                }
                result
            }
        }
    }

    pub fn get_injected_text(&self, output_line_index: usize, output_offset: usize) -> Option<InjectedText> {
        let offset = self.output_position_to_offset_in_input_with_injections(output_line_index, output_offset);
        let injected_text = self.get_injected_text_at_offset(offset)?;
        let (_, injection_options) = self.injections()?;
        Some(InjectedText { options: injection_options[injected_text.injected_text_index].clone() })
    }

    fn get_injected_text_at_offset(&self, offset_in_input_with_injections: usize) -> Option<InjectedTextAtOffset> {
        let (injection_offsets, injection_options) = self.injections()?;

        let mut total_injected_text_length_before = 0usize;
        for (i, (injection_offset, options)) in injection_offsets.iter().zip(injection_options).enumerate() {
            let length = injected_length(options);
            let injected_text_start_offset_in_input_with_injections = injection_offset + total_injected_text_length_before;
            let injected_text_end_offset_in_input_with_injections =
                injection_offset + total_injected_text_length_before + length;

            if injected_text_start_offset_in_input_with_injections > offset_in_input_with_injections {
                // Injected text starts later.
                break; // All later injected texts have an even larger offset.
            }

            if offset_in_input_with_injections <= injected_text_end_offset_in_input_with_injections {
                // Injected text ends after or with the given position (but also starts with or before it).
                return Some(InjectedTextAtOffset {
                    injected_text_index: i,
                    offset_in_input_with_injections: injected_text_start_offset_in_input_with_injections,
                    length,
                });
            }

            total_injected_text_length_before += length;
        }

        None
    }
}

fn has_right_cursor_stop(cursor_stop: Option<InjectedTextCursorStops>) -> bool {
    match cursor_stop {
        None => true,
        Some(stop) => matches!(stop, InjectedTextCursorStops::Right | InjectedTextCursorStops::Both),
    }
}

fn has_left_cursor_stop(cursor_stop: Option<InjectedTextCursorStops>) -> bool {
    match cursor_stop {
        None => true,
        Some(stop) => matches!(stop, InjectedTextCursorStops::Left | InjectedTextCursorStops::Both),
    }
}

/// `InjectedText`.
#[derive(Clone, Debug, PartialEq)]
pub struct InjectedText {
    pub options: InjectedTextOptions,
}

/// `OutputPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputPosition {
    pub output_line_index: usize,
    pub output_offset: usize,
}

impl OutputPosition {
    pub fn new(output_line_index: usize, output_offset: usize) -> Self {
        Self { output_line_index, output_offset }
    }

    pub fn to_position(&self, base_line_number: i32) -> Position {
        Position::new(base_line_number + self.output_line_index as i32, self.output_offset as i32 + 1)
    }
}

/// `toString()`.
impl fmt::Display for OutputPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.output_line_index, self.output_offset)
    }
}

/// `wordBreak: 'normal' | 'keepAll'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordBreak {
    Normal,
    KeepAll,
}

/// `ILineBreaksComputerContext`.
pub trait LineBreaksComputerContext {
    fn get_line_content(&self, line_number: i32) -> JsString;
    fn get_line_injected_text(&self, line_number: i32) -> Option<Vec<LineInjectedText>>;
}

/// `ILineBreaksComputerFactory`.
pub trait LineBreaksComputerFactory {
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
    ) -> Box<dyn LineBreaksComputer + 'a>;
}

/// `ILineBreaksComputer`.
pub trait LineBreaksComputer {
    /// Pass in `previous_line_break_data` if the only difference is in breaking columns!!!
    fn add_request(&mut self, line_number: i32, previous_line_break_data: Option<ModelLineProjectionData>);
    fn finalize(&mut self) -> Vec<Option<ModelLineProjectionData>>;
}
