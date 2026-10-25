//! Porte de `src/vs/editor/common/services/semanticTokensProviderStyling.ts`: o estilo que o tema dá aos tokens
//! semânticos de um provedor (`SemanticTokensProviderStyling`) e a conversão dos tokens do provedor em tokens
//! esparsos do modelo (`toMultilineTokens2`).
//!
//! Diferenças do original, nenhuma de comportamento:
//! - A `HashTable` própria (lista encadeada com tamanhos primos que cresce a 2/3) é um `HashMap` com a mesma chave
//!   `(tokenTypeIndex, tokenModifierSet, languageId)`: ela só guarda o metadado já calculado.
//! - `IThemeService`, `ILanguageService` e `ILogService` são os `Rc` dos contratos do porte; `ENABLE_TRACE` é falso no
//!   original e o ramo de rastreio não existe aqui.
//! - O `Uint32Array` é um `Vec<u32>`. A aritmética que o original faz em `number` e converte com `| 0` usa `i64` com o
//!   corte para `i32` no mesmo ponto; o que vai para o `Uint32Array` de destino é truncado módulo 2^32 (`as u32`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_platform::log::{LogServiceRef, Logger};

use crate::encoded_token_attributes::{font_style, metadata_consts};
use crate::language_features_service::{SemanticTokens, SemanticTokensLegend};
use crate::language_service::ILanguageService;
use crate::services::theme_service::IThemeService;
use crate::sparse_multiline_tokens::SparseMultilineTokens;

/// `SemanticTokensProviderStylingConstants.NO_STYLING`.
pub const NO_STYLING: u32 = 0b0111_1111_1111_1111_1111_1111_1111_1111;

/// `SemanticTokensProviderStyling`.
pub struct SemanticTokensProviderStyling {
    legend: SemanticTokensLegend,
    theme_service: Rc<dyn IThemeService>,
    language_service: Rc<dyn ILanguageService>,
    log_service: LogServiceRef,
    hash_table: RefCell<HashMap<(u32, u32, u32), u32>>,
    has_warned_overlapping_tokens: Cell<bool>,
    has_warned_invalid_length_tokens: Cell<bool>,
    has_warned_invalid_edit_start: Cell<bool>,
    has_warned_invalid_edit_delete_count: Cell<bool>,
}

impl SemanticTokensProviderStyling {
    pub fn new(
        legend: SemanticTokensLegend,
        theme_service: Rc<dyn IThemeService>,
        language_service: Rc<dyn ILanguageService>,
        log_service: LogServiceRef,
    ) -> Self {
        Self {
            legend,
            theme_service,
            language_service,
            log_service,
            hash_table: RefCell::new(HashMap::new()),
            has_warned_overlapping_tokens: Cell::new(false),
            has_warned_invalid_length_tokens: Cell::new(false),
            has_warned_invalid_edit_start: Cell::new(false),
            has_warned_invalid_edit_delete_count: Cell::new(false),
        }
    }

    /// `getMetadata(tokenTypeIndex, tokenModifierSet, languageId)`.
    pub fn get_metadata(&self, token_type_index: u32, token_modifier_set: u32, language_id: &str) -> u32 {
        let encoded_language_id = self.language_service.language_id_codec().encode_language_id(language_id);
        let key = (token_type_index, token_modifier_set, encoded_language_id);
        if let Some(metadata) = self.hash_table.borrow().get(&key) {
            return *metadata;
        }

        let metadata;
        match self.legend.token_types.get(token_type_index as usize) {
            Some(token_type) => {
                let mut token_modifiers: Vec<String> = Vec::new();
                let mut modifier_set = token_modifier_set;
                let mut modifier_index = 0;
                while modifier_set > 0 && modifier_index < self.legend.token_modifiers.len() {
                    if modifier_set & 1 != 0 {
                        token_modifiers.push(self.legend.token_modifiers[modifier_index].clone());
                    }
                    modifier_set >>= 1;
                    modifier_index += 1;
                }

                let token_style = self.theme_service.get_color_theme().get_token_style_metadata(token_type, &token_modifiers, language_id);
                metadata = match token_style {
                    None => NO_STYLING,
                    Some(token_style) => {
                        let mut metadata = 0;
                        if let Some(italic) = token_style.italic {
                            let italic_bit = (if italic { font_style::ITALIC } else { 0 }) << metadata_consts::FONT_STYLE_OFFSET;
                            metadata |= italic_bit | metadata_consts::SEMANTIC_USE_ITALIC;
                        }
                        if let Some(bold) = token_style.bold {
                            let bold_bit = (if bold { font_style::BOLD } else { 0 }) << metadata_consts::FONT_STYLE_OFFSET;
                            metadata |= bold_bit | metadata_consts::SEMANTIC_USE_BOLD;
                        }
                        if let Some(underline) = token_style.underline {
                            let underline_bit = (if underline { font_style::UNDERLINE } else { 0 }) << metadata_consts::FONT_STYLE_OFFSET;
                            metadata |= underline_bit | metadata_consts::SEMANTIC_USE_UNDERLINE;
                        }
                        if let Some(strikethrough) = token_style.strikethrough {
                            let strikethrough_bit = (if strikethrough { font_style::STRIKETHROUGH } else { 0 }) << metadata_consts::FONT_STYLE_OFFSET;
                            metadata |= strikethrough_bit | metadata_consts::SEMANTIC_USE_STRIKETHROUGH;
                        }
                        if let Some(foreground) = token_style.foreground.filter(|foreground| *foreground != 0) {
                            let foreground_bits = foreground << metadata_consts::FOREGROUND_OFFSET;
                            metadata |= foreground_bits | metadata_consts::SEMANTIC_USE_FOREGROUND;
                        }
                        if metadata == 0 {
                            // Nothing!
                            NO_STYLING
                        } else {
                            metadata
                        }
                    }
                };
            }
            None => metadata = NO_STYLING,
        }
        self.hash_table.borrow_mut().insert(key, metadata);
        metadata
    }

    /// `warnOverlappingSemanticTokens(lineNumber, startColumn)`.
    pub fn warn_overlapping_semantic_tokens(&self, line_number: i64, start_column: i64) {
        if !self.has_warned_overlapping_tokens.replace(true) {
            self.log_service.warn(&format!("Overlapping semantic tokens detected at lineNumber {line_number}, column {start_column}"));
        }
    }

    /// `warnInvalidLengthSemanticTokens(lineNumber, startColumn)`.
    pub fn warn_invalid_length_semantic_tokens(&self, line_number: i64, start_column: i64) {
        if !self.has_warned_invalid_length_tokens.replace(true) {
            self.log_service.warn(&format!("Semantic token with invalid length detected at lineNumber {line_number}, column {start_column}"));
        }
    }

    /// `warnInvalidEditStart(previousResultId, resultId, editIndex, editStart, maxExpectedStart)`.
    pub fn warn_invalid_edit_start(&self, previous_result_id: Option<&str>, result_id: Option<&str>, edit_index: usize, edit_start: u32, max_expected_start: usize) {
        if !self.has_warned_invalid_edit_start.replace(true) {
            self.log_service.warn(&format!(
                "Invalid semantic tokens edit detected (previousResultId: {}, resultId: {}) at edit #{edit_index}: The provided start offset {edit_start} is outside the previous data (length {max_expected_start}).",
                previous_result_id.unwrap_or("undefined"),
                result_id.unwrap_or("undefined"),
            ));
        }
    }

    /// `warnInvalidEditDeleteCount(previousResultId, resultId, srcLength, deltaLength)`.
    pub fn warn_invalid_edit_delete_count(&self, previous_result_id: Option<&str>, result_id: Option<&str>, src_length: usize, delta_length: i64) {
        if !self.has_warned_invalid_edit_delete_count.replace(true) {
            self.log_service.warn(&format!(
                "Invalid semantic tokens edit detected (previousResultId: {}, resultId: {}): The edits delete more tokens than the previous data contains (previous length {src_length}, computed length {}).",
                previous_result_id.unwrap_or("undefined"),
                result_id.unwrap_or("undefined"),
                src_length as i64 + delta_length,
            ));
        }
    }
}

/// `SemanticColoringConstants.DesiredTokensPerArea`: buffers de 8KB se possível, `8192 / (5 * 4)` tokens por área.
const DESIRED_TOKENS_PER_AREA: usize = 400;

/// `SemanticColoringConstants.DesiredMaxAreas`: no máximo 1024 áreas se possível, compensando com mais tokens por área.
const DESIRED_MAX_AREAS: usize = 1024;

/// `x | 0` sobre um `number` inteiro: o corte para `int32`.
fn to_int32(value: i64) -> i64 {
    i64::from(value as i32)
}

/// `toMultilineTokens2(tokens, styling, languageId)`.
pub fn to_multiline_tokens2(tokens: &SemanticTokens, styling: &SemanticTokensProviderStyling, language_id: &str) -> Vec<SparseMultilineTokens> {
    let src_data = &tokens.data;
    let token_count = src_data.len() / 5;
    let tokens_per_area = token_count.div_ceil(DESIRED_MAX_AREAS).max(DESIRED_TOKENS_PER_AREA);
    let mut result: Vec<SparseMultilineTokens> = Vec::new();

    let mut token_index = 0;
    let mut last_line_number: i64 = 1;
    let mut last_start_character: i64 = 0;
    while token_index < token_count {
        let token_start_index = token_index;
        let mut token_end_index = (token_start_index + tokens_per_area).min(token_count);

        // Keep tokens on the same line in the same area...
        if token_end_index < token_count {
            let mut small_token_end_index = token_end_index;
            while small_token_end_index - 1 > token_start_index && src_data[5 * small_token_end_index] == 0 {
                small_token_end_index -= 1;
            }

            if small_token_end_index - 1 == token_start_index {
                // there are so many tokens on this line that our area would be empty, we must now go right
                let mut big_token_end_index = token_end_index;
                while big_token_end_index + 1 < token_count && src_data[5 * big_token_end_index] == 0 {
                    big_token_end_index += 1;
                }
                token_end_index = big_token_end_index;
            } else {
                token_end_index = small_token_end_index;
            }
        }

        let mut dest_data: Vec<u32> = vec![0; (token_end_index - token_start_index) * 4];
        let mut dest_offset = 0;
        let mut area_line: i64 = 0;
        let mut prev_line_number: i64 = 0;
        let mut prev_end_character: i64 = 0;
        while token_index < token_end_index {
            let src_offset = 5 * token_index;
            let delta_line = i64::from(src_data[src_offset]);
            let delta_character = i64::from(src_data[src_offset + 1]);
            // Casting both `lineNumber`, `startCharacter` and `endCharacter` here to int32 using `|0`
            // to validate below with the actual values that will be inserted in the Uint32Array result
            let line_number = to_int32(last_line_number + delta_line);
            let start_character = if delta_line == 0 { to_int32(last_start_character + delta_character) } else { delta_character };
            let length = i64::from(src_data[src_offset + 2]);
            let end_character = to_int32(start_character + length);
            let token_type_index = src_data[src_offset + 3];
            let token_modifier_set = src_data[src_offset + 4];

            if end_character <= start_character {
                // this token is invalid (most likely a negative length casted to uint32)
                styling.warn_invalid_length_semantic_tokens(line_number, start_character + 1);
            } else if prev_line_number == line_number && prev_end_character > start_character {
                // this token overlaps with the previous token
                styling.warn_overlapping_semantic_tokens(line_number, start_character + 1);
            } else {
                let metadata = styling.get_metadata(token_type_index, token_modifier_set, language_id);

                if metadata != NO_STYLING {
                    if area_line == 0 {
                        area_line = line_number;
                    }
                    dest_data[dest_offset] = (line_number - area_line) as u32;
                    dest_data[dest_offset + 1] = start_character as u32;
                    dest_data[dest_offset + 2] = end_character as u32;
                    dest_data[dest_offset + 3] = metadata;
                    dest_offset += 4;

                    prev_line_number = line_number;
                    prev_end_character = end_character;
                }
            }

            last_line_number = line_number;
            last_start_character = start_character;
            token_index += 1;
        }

        dest_data.truncate(dest_offset);

        result.push(SparseMultilineTokens::create(area_line as i32, dest_data));
    }

    result
}
