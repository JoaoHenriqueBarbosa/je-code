//! Porte de `src/vs/editor/browser/viewParts/minimap/minimapCharSheet.ts`: as constantes da folha de
//! caracteres do minimap e o mapeamento de código de caractere para índice.
//!
//! Divergências:
//! - O `const enum Constants` vira o módulo [`constants`] com constantes `usize`; `RGBA_SAMPLED_ROW_WIDTH` e
//!   `CHAR_COUNT` são calculadas como no original.
//! - `allCharCodes` (um `ReadonlyArray` calculado uma vez) é a função [`all_char_codes`].
//! - `getCharIndex` recebe o código como `u32` (um código de caractere nunca é negativo), então a conta
//!   `(chCode + CHAR_COUNT) % CHAR_COUNT` do original, que só vê valores de -32 a -1 antes da soma, dá o
//!   mesmo resultado em `i64`.

/// `Constants`.
pub mod constants {
    /// Espaço.
    pub const START_CH_CODE: u32 = 32;
    /// Til (`~`).
    pub const END_CH_CODE: u32 = 126;
    /// O marcador de posição UTF.
    pub const UNKNOWN_CODE: u32 = 65533;
    pub const CHAR_COUNT: usize = (END_CH_CODE - START_CH_CODE) as usize + 2;

    pub const SAMPLED_CHAR_HEIGHT: usize = 16;
    pub const SAMPLED_CHAR_WIDTH: usize = 10;

    pub const BASE_CHAR_HEIGHT: usize = 2;
    pub const BASE_CHAR_WIDTH: usize = 1;

    pub const RGBA_CHANNELS_CNT: usize = 4;
    pub const RGBA_SAMPLED_ROW_WIDTH: usize = RGBA_CHANNELS_CNT * CHAR_COUNT * SAMPLED_CHAR_WIDTH;
}

/// `allCharCodes`: de `START_CH_CODE` a `END_CH_CODE`, mais `UNKNOWN_CODE`.
pub fn all_char_codes() -> Vec<u32> {
    let mut codes: Vec<u32> = (constants::START_CH_CODE..=constants::END_CH_CODE).collect();
    codes.push(constants::UNKNOWN_CODE);
    codes
}

/// `getCharIndex(chCode, fontScale)`.
pub fn get_char_index(ch_code: u32, font_scale: f64) -> usize {
    let ch_code = i64::from(ch_code) - i64::from(constants::START_CH_CODE);
    let char_count = constants::CHAR_COUNT as i64;
    // O `>` (e não `>=`) é do original: `CHAR_COUNT` sai como índice, uma posição além da folha.
    if ch_code < 0 || ch_code > char_count {
        if font_scale <= 2.0 {
            // para escalas menores, qualquer caractere ASCII serve...
            return ((ch_code + char_count) % char_count) as usize;
        }
        return constants::CHAR_COUNT - 1; // símbolo desconhecido
    }

    ch_code as usize
}
