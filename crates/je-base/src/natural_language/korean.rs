//! Porte de `src/vs/base/common/naturalLanguage/korean.ts`: os caracteres ASCII com que um Hangul teria
//! sido digitado num teclado qwerty (o 2-Set coreano), para o `matchesWords` achar texto em coreano
//! digitando em latim.
//!
//! Cobre só o Hangul moderno (as sílabas, as jamo modernas e as jamo de compatibilidade), não o arcaico.
//! No original os dados são `Uint8Array`/`Uint16Array` com até duas letras por entrada, empacotadas como
//! `segunda << 8 | primeira`; aqui são `[u16; N]` montados por [`c`] e [`p`], e o resultado é um `Vec<u16>`
//! de códigos ASCII (o `Uint32Array` do original, o único consumidor compara com `charCodeAt`).

const fn c(first: u8) -> u16 {
    first as u16
}

/// Duas letras numa entrada: `second << 8 | first`.
const fn p(first: u8, second: u8) -> u16 {
    ((second as u16) << 8) | first as u16
}

/// Códigos ASCII que a unidade `code` teria sido digitada com, ou `None` se não for Hangul moderno.
pub fn get_korean_alt_chars(code: u32) -> Option<Vec<u16>> {
    let result = disassemble_korean(code);
    if result.is_empty() { None } else { Some(result) }
}

const INITIAL_CONSONANT: u32 = 0x1100;
const VOWEL: u32 = 0x1161;
const FINAL_CONSONANT: u32 = 0x11A8;
const COMPATIBILITY_JAMO_START: u32 = 0x3131;

fn disassemble_korean(code: u32) -> Vec<u16> {
    let mut buffer = Vec::with_capacity(6);

    // Consoantes iniciais (초성)
    get_codes_from_array(code, &MODERN_CONSONANTS, INITIAL_CONSONANT, &mut buffer);
    if !buffer.is_empty() {
        return buffer;
    }

    // Vogais (중성)
    get_codes_from_array(code, &MODERN_VOWELS, VOWEL, &mut buffer);
    if !buffer.is_empty() {
        return buffer;
    }

    // Consoantes finais (종성)
    get_codes_from_array(code, &MODERN_FINAL_CONSONANTS, FINAL_CONSONANT, &mut buffer);
    if !buffer.is_empty() {
        return buffer;
    }

    // Hangul Compatibility Jamo
    get_codes_from_array(code, &COMPATIBILITY_JAMO, COMPATIBILITY_JAMO_START, &mut buffer);
    if !buffer.is_empty() {
        return buffer;
    }

    // Hangul Syllables
    if (0xAC00..=0xD7A3).contains(&code) {
        let hangul_index = code - 0xAC00;
        let vowel_and_final_consonant_product = hangul_index % 588;

        // Base 0 começando em 0x1100
        let initial_consonant_index = hangul_index / 588;
        // Base 0 começando em 0x1161
        let vowel_index = vowel_and_final_consonant_product / 28;
        // Base 0 começando em 0x11A8. Subtrai 1 porque o algoritmo padrão usa o índice 0 para "sem
        // consoante final".
        let final_consonant_index = (vowel_and_final_consonant_product % 28) as i64 - 1;

        let compatibility_len = COMPATIBILITY_JAMO.len() as i64;
        let beyond = |start: u32, index: u32| i64::from(start) + i64::from(index) - i64::from(COMPATIBILITY_JAMO_START) < compatibility_len;
        if (initial_consonant_index as usize) < MODERN_CONSONANTS.len() {
            get_codes_from_array(initial_consonant_index, &MODERN_CONSONANTS, 0, &mut buffer);
        } else if beyond(INITIAL_CONSONANT, initial_consonant_index) {
            get_codes_from_array(INITIAL_CONSONANT + initial_consonant_index, &COMPATIBILITY_JAMO, COMPATIBILITY_JAMO_START, &mut buffer);
        }

        if (vowel_index as usize) < MODERN_VOWELS.len() {
            get_codes_from_array(vowel_index, &MODERN_VOWELS, 0, &mut buffer);
        } else if beyond(VOWEL, vowel_index) {
            get_codes_from_array((VOWEL + vowel_index).saturating_sub(COMPATIBILITY_JAMO_START), &COMPATIBILITY_JAMO, COMPATIBILITY_JAMO_START, &mut buffer);
        }

        if final_consonant_index >= 0 {
            let final_consonant_index = final_consonant_index as u32;
            if (final_consonant_index as usize) < MODERN_FINAL_CONSONANTS.len() {
                get_codes_from_array(final_consonant_index, &MODERN_FINAL_CONSONANTS, 0, &mut buffer);
            } else if beyond(FINAL_CONSONANT, final_consonant_index) {
                get_codes_from_array(
                    (FINAL_CONSONANT + final_consonant_index).saturating_sub(COMPATIBILITY_JAMO_START),
                    &COMPATIBILITY_JAMO,
                    COMPATIBILITY_JAMO_START,
                    &mut buffer,
                );
            }
        }
    }
    buffer
}

fn get_codes_from_array(code: u32, array: &[u16], array_start_index: u32, buffer: &mut Vec<u16>) {
    // Confere se o código está dentro do intervalo do array
    if code >= array_start_index && code < array_start_index + array.len() as u32 {
        add_codes_to_buffer(array[(code - array_start_index) as usize], buffer);
    }
}

fn add_codes_to_buffer(codes: u16, buffer: &mut Vec<u16>) {
    // NUL é ignorado, o que dispensa um mapa para os dados de caracteres arcaicos
    if codes == 0 {
        return;
    }
    // Número guardado no formato: terceiro código opcional << 16 | segundo código opcional << 8 | código
    buffer.push(codes & 0xFF);
    if codes >> 8 != 0 {
        buffer.push((codes >> 8) & 0xFF);
    }
}

const MODERN_CONSONANTS: [u16; 19] = [
    c(b'r'), // ㄱ
    c(b'R'), // ㄲ
    c(b's'), // ㄴ
    c(b'e'), // ㄷ
    c(b'E'), // ㄸ
    c(b'f'), // ㄹ
    c(b'a'), // ㅁ
    c(b'q'), // ㅂ
    c(b'Q'), // ㅃ
    c(b't'), // ㅅ
    c(b'T'), // ㅆ
    c(b'd'), // ㅇ
    c(b'w'), // ㅈ
    c(b'W'), // ㅉ
    c(b'c'), // ㅊ
    c(b'z'), // ㅋ
    c(b'x'), // ㅌ
    c(b'v'), // ㅍ
    c(b'g'), // ㅎ
];

const MODERN_VOWELS: [u16; 21] = [
    c(b'k'), // -> ㅏ
    c(b'o'), // -> ㅐ
    c(b'i'), // -> ㅑ
    c(b'O'), // -> ㅒ
    c(b'j'), // -> ㅓ
    c(b'p'), // -> ㅔ
    c(b'u'), // -> ㅕ
    c(b'P'), // -> ㅖ
    c(b'h'), // -> ㅗ
    p(b'h', b'k'), // -> ㅘ
    p(b'h', b'o'), // -> ㅙ
    p(b'h', b'l'), // -> ㅚ
    c(b'y'), // -> ㅛ
    c(b'n'), // -> ㅜ
    p(b'n', b'j'), // -> ㅝ
    p(b'n', b'p'), // -> ㅞ
    p(b'n', b'l'), // -> ㅟ
    c(b'b'), // -> ㅠ
    c(b'm'), // -> ㅡ
    p(b'm', b'l'), // -> ㅢ
    c(b'l'), // -> ㅣ
];

const MODERN_FINAL_CONSONANTS: [u16; 27] = [
    c(b'r'), // ㄱ
    c(b'R'), // ㄲ
    p(b'r', b't'), // ㄳ
    c(b's'), // ㄴ
    p(b's', b'w'), // ㄵ
    p(b's', b'g'), // ㄶ
    c(b'e'), // ㄷ
    c(b'f'), // ㄹ
    p(b'f', b'r'), // ㄺ
    p(b'f', b'a'), // ㄻ
    p(b'f', b'q'), // ㄼ
    p(b'f', b't'), // ㄽ
    p(b'f', b'x'), // ㄾ
    p(b'f', b'v'), // ㄿ
    p(b'f', b'g'), // ㅀ
    c(b'a'), // ㅁ
    c(b'q'), // ㅂ
    p(b'q', b't'), // ㅄ
    c(b't'), // ㅅ
    c(b'T'), // ㅆ
    c(b'd'), // ㅇ
    c(b'w'), // ㅈ
    c(b'c'), // ㅊ
    c(b'z'), // ㅋ
    c(b'x'), // ㅌ
    c(b'v'), // ㅍ
    c(b'g'), // ㅎ
];

const COMPATIBILITY_JAMO: [u16; 51] = [
    c(b'r'), // ㄱ
    c(b'R'), // ㄲ
    p(b'r', b't'), // ㄳ
    c(b's'), // ㄴ
    p(b's', b'w'), // ㄵ
    p(b's', b'g'), // ㄶ
    c(b'e'), // ㄷ
    c(b'E'), // ㄸ
    c(b'f'), // ㄹ
    p(b'f', b'r'), // ㄺ
    p(b'f', b'a'), // ㄻ
    p(b'f', b'q'), // ㄼ
    p(b'f', b't'), // ㄽ
    p(b'f', b'x'), // ㄾ
    p(b'f', b'v'), // ㄿ
    p(b'f', b'g'), // ㅀ
    c(b'a'), // ㅁ
    c(b'q'), // ㅂ
    c(b'Q'), // ㅃ
    p(b'q', b't'), // ㅄ
    c(b't'), // ㅅ
    c(b'T'), // ㅆ
    c(b'd'), // ㅇ
    c(b'w'), // ㅈ
    c(b'W'), // ㅉ
    c(b'c'), // ㅊ
    c(b'z'), // ㅋ
    c(b'x'), // ㅌ
    c(b'v'), // ㅍ
    c(b'g'), // ㅎ
    c(b'k'), // ㅏ
    c(b'o'), // ㅐ
    c(b'i'), // ㅑ
    c(b'O'), // ㅒ
    c(b'j'), // ㅓ
    c(b'p'), // ㅔ
    c(b'u'), // ㅕ
    c(b'P'), // ㅖ
    c(b'h'), // ㅗ
    p(b'h', b'k'), // ㅘ
    p(b'h', b'o'), // ㅙ
    p(b'h', b'l'), // ㅚ
    c(b'y'), // ㅛ
    c(b'n'), // ㅜ
    p(b'n', b'j'), // ㅝ
    p(b'n', b'p'), // ㅞ
    p(b'n', b'l'), // ㅟ
    c(b'b'), // ㅠ
    c(b'm'), // ㅡ
    p(b'm', b'l'), // ㅢ
    c(b'l'), // ㅣ
];
