//! Porte de `src/vs/editor/common/core/wordCharacterClassifier.ts`.
//!
//! Divergência: o `Intl.Segmenter` do original (palavras por locale: chinês, japonês, tailandês) ainda não
//! existe no porte, então com `intl_segmenter_locales` vazio (o caso de toda a busca no buffer) tudo é
//! igual, e com locales o `findPrevIntlWordBeforeOrAtOffset`/`findNextIntlWordAtOrAfterOffset` devolvem
//! `None` como se o locale não tivesse segmentos.
// TODO(integração): trocar pelo `icu_segmenter` (mesma versão do zjsc) quando o `Intl.Segmenter` entrar no je-base.

use std::rc::Rc;

use je_base::char_code;
use je_base::map::LRUCache;

use crate::character_classifier::CharacterClassifier;

/// `WordCharacterClass`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WordCharacterClass {
    Regular = 0,
    Whitespace = 1,
    WordSeparator = 2,
}

/// Um segmento de palavra do `Intl.Segmenter` (`IntlWordSegmentData`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntlWordSegmentData {
    pub segment: Vec<u16>,
    pub index: usize,
}

/// Classifica cada caractere como regular, espaço ou separador de palavra.
#[derive(Clone, Debug)]
pub struct WordCharacterClassifier {
    classifier: CharacterClassifier,
    pub intl_segmenter_locales: Vec<String>,
}

impl WordCharacterClassifier {
    pub fn new(word_separators: &[u16], intl_segmenter_locales: Vec<String>) -> Self {
        let mut classifier = CharacterClassifier::new(WordCharacterClass::Regular as u8);
        for &unit in word_separators {
            classifier.set(i32::from(unit), WordCharacterClass::WordSeparator as u8);
        }
        classifier.set(i32::from(char_code::Space), WordCharacterClass::Whitespace as u8);
        classifier.set(i32::from(char_code::Tab), WordCharacterClass::Whitespace as u8);
        Self {
            classifier,
            intl_segmenter_locales,
        }
    }

    pub fn get(&self, char_code: i32) -> WordCharacterClass {
        match self.classifier.get(char_code) {
            1 => WordCharacterClass::Whitespace,
            2 => WordCharacterClass::WordSeparator,
            _ => WordCharacterClass::Regular,
        }
    }

    /// `findPrevIntlWordBeforeOrAtOffset`: sem segmentador, nunca acha.
    pub fn find_prev_intl_word_before_or_at_offset(&self, _line: &[u16], _offset: usize) -> Option<IntlWordSegmentData> {
        None
    }

    /// `findNextIntlWordAtOrAfterOffset`: sem segmentador, nunca acha.
    pub fn find_next_intl_word_at_or_after_offset(&self, _line_content: &[u16], _offset: usize) -> Option<IntlWordSegmentData> {
        None
    }
}

thread_local! {
    static WORD_CLASSIFIER_CACHE: LRUCache<String, Rc<WordCharacterClassifier>> = LRUCache::new(10, 1.0);
}

/// `getMapForWordSeparators`: um classificador por combinação de separadores e locales, num LRU de 10.
pub fn get_map_for_word_separators(word_separators: &[u16], intl_segmenter_locales: &[String]) -> Rc<WordCharacterClassifier> {
    let key = format!("{}/{}", String::from_utf16_lossy(word_separators), intl_segmenter_locales.join(","));
    WORD_CLASSIFIER_CACHE.with(|cache| {
        if let Some(found) = cache.get(&key) {
            return found;
        }
        let result = Rc::new(WordCharacterClassifier::new(word_separators, intl_segmenter_locales.to_vec()));
        cache.set(key, result.clone());
        result
    })
}
