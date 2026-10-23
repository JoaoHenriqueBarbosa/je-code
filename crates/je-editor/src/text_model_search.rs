//! O trecho de `src/vs/editor/common/model/textModelSearch.ts` de que o buffer depende: o `Searcher`
//! (varredura com `lastIndex`), `isValidMatch` (fronteira de palavra) e `createFindMatch`.
//!
//! Fica de fora o resto do arquivo (`SearchParams`, `TextModelSearch`): dependem do `TextModel` e do
//! `createRegExp` do `strings.ts`, que ainda não foram portados.

use je_base::JsString;
use je_base::char_code;
use je_base::strings;
use zyarr::Match;

use crate::js_reg_exp::JsRegExp;
use crate::model::FindMatch;
use crate::range::Range;
use crate::word_character_classifier::{WordCharacterClass, WordCharacterClassifier};

/// `createFindMatch(range, rawMatches, captureMatches)`: `text` é o texto sobre o qual `raw_matches` foi achado.
pub fn create_find_match(range: Range, text: &[u16], raw_matches: &Match, capture_matches: bool) -> FindMatch {
    if !capture_matches {
        return FindMatch { range, matches: None };
    }
    let matches = raw_matches
        .captures
        .iter()
        .map(|capture| capture.as_ref().map(|span| JsString::from(&text[span.clone()])))
        .collect();
    FindMatch {
        range,
        matches: Some(matches),
    }
}

fn left_is_word_boundary(word_separators: &WordCharacterClassifier, text: &[u16], match_start_index: usize, match_length: usize) -> bool {
    if match_start_index == 0 {
        // O casamento começa no início da cadeia.
        return true;
    }

    let char_before = text[match_start_index - 1];
    if word_separators.get(i32::from(char_before)) != WordCharacterClass::Regular {
        // O caractere antes do casamento é um separador de palavra.
        return true;
    }

    if char_before == char_code::CarriageReturn || char_before == char_code::LineFeed {
        // O caractere antes do casamento é uma quebra de linha.
        return true;
    }

    if match_length > 0 {
        let first_char_in_match = text[match_start_index];
        if word_separators.get(i32::from(first_char_in_match)) != WordCharacterClass::Regular {
            // O primeiro caractere do casamento é um separador de palavra.
            return true;
        }
    }

    false
}

fn right_is_word_boundary(word_separators: &WordCharacterClassifier, text: &[u16], match_start_index: usize, match_length: usize) -> bool {
    if match_start_index + match_length == text.len() {
        // O casamento termina no fim da cadeia.
        return true;
    }

    let char_after = text[match_start_index + match_length];
    if word_separators.get(i32::from(char_after)) != WordCharacterClass::Regular {
        // O caractere depois do casamento é um separador de palavra.
        return true;
    }

    if char_after == char_code::CarriageReturn || char_after == char_code::LineFeed {
        // O caractere depois do casamento é uma quebra de linha.
        return true;
    }

    if match_length > 0 {
        let last_char_in_match = text[match_start_index + match_length - 1];
        if word_separators.get(i32::from(last_char_in_match)) != WordCharacterClass::Regular {
            // O último caractere do casamento é um separador de palavra.
            return true;
        }
    }

    false
}

/// `isValidMatch`: o casamento está colado em fronteiras de palavra dos dois lados. O `textLength` do
/// original é sempre `text.length` e some da assinatura.
pub fn is_valid_match(word_separators: &WordCharacterClassifier, text: &[u16], match_start_index: usize, match_length: usize) -> bool {
    left_is_word_boundary(word_separators, text, match_start_index, match_length)
        && right_is_word_boundary(word_separators, text, match_start_index, match_length)
}

/// `Searcher`: varre um texto casamento a casamento, sem repetir o mesmo intervalo.
pub struct Searcher<'a> {
    pub word_separators: Option<&'a WordCharacterClassifier>,
    search_regex: &'a JsRegExp,
    prev_match_start_index: isize,
    prev_match_length: usize,
}

impl<'a> Searcher<'a> {
    pub fn new(word_separators: Option<&'a WordCharacterClassifier>, search_regex: &'a JsRegExp) -> Self {
        Self {
            word_separators,
            search_regex,
            prev_match_start_index: -1,
            prev_match_length: 0,
        }
    }

    pub fn reset(&mut self, last_index: usize) {
        self.search_regex.set_last_index(last_index);
        self.prev_match_start_index = -1;
        self.prev_match_length = 0;
    }

    pub fn next(&mut self, text: &[u16]) -> Option<Match> {
        let text_length = text.len();

        loop {
            if self.prev_match_start_index + self.prev_match_length as isize == text_length as isize {
                // Chegou ao fim da linha.
                return None;
            }

            let m = self.search_regex.exec(text)?;

            let match_start_index = m.start();
            let match_length = m.end() - m.start();
            if match_start_index as isize == self.prev_match_start_index && match_length == self.prev_match_length {
                if match_length == 0 {
                    // O resultado é a cadeia vazia e não avança `lastIndex`, então o `exec` ficaria preso.
                    // Recupera avançando dois se houver um par substituto e um nos demais casos.
                    let last_index = self.search_regex.last_index();
                    let code_point = strings::get_next_code_point(text, text_length as i64, last_index as i64);
                    if code_point != strings::NAN_CODE_POINT && code_point > 0xFFFF {
                        self.search_regex.set_last_index(last_index + 2);
                    } else {
                        self.search_regex.set_last_index(last_index + 1);
                    }
                    continue;
                }
                // Sai cedo se a regex casa o mesmo intervalo duas vezes.
                return None;
            }
            self.prev_match_start_index = match_start_index as isize;
            self.prev_match_length = match_length;

            match self.word_separators {
                Some(word_separators) if !is_valid_match(word_separators, text, match_start_index, match_length) => {}
                _ => return Some(m),
            }
        }
    }
}
