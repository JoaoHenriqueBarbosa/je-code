//! Auxiliares sobre linhas em UTF-16: ASCII, expansão de tabulação e as classes de palavra.

use std::rc::Rc;

use je_base::strings::is_full_width_character;
use je_editor::word_character_classifier::{WordCharacterClass, WordCharacterClassifier, get_map_for_word_separators};
use je_editor::word_helper::USUAL_WORD_SEPARATORS;

/// A linha só tem ASCII (nenhum caractere largo, nenhum par substituto): a grade monoespaçada vale.
pub fn is_basic_ascii(units: &[u16]) -> bool {
    units.iter().all(|&u| u < 0x80)
}

/// Quantas células um caractere ocupa a partir da coluna visual `visual_column` (a tabulação vai até a
/// próxima parada; caractere largo ocupa duas).
pub fn cells_of(unit: u16, visual_column: i32, tab_size: i32) -> i32 {
    if unit == 9 {
        je_editor::cursor_columns::next_render_tab_stop(visual_column, tab_size) - visual_column
    } else if unit >= 0x1100 && is_full_width_character(unit) {
        2
    } else {
        1
    }
}

/// O texto de um trecho como a view o desenha: tabulação vira espaços até a próxima parada, contando a
/// partir de `visual_column`. Devolve também a coluna visual no fim do trecho.
pub fn expand_tabs(units: &[u16], mut visual_column: i32, tab_size: i32) -> (String, i32) {
    let mut out = String::with_capacity(units.len());
    let mut pending: Vec<u16> = Vec::new();
    let flush = |pending: &mut Vec<u16>, out: &mut String| {
        if !pending.is_empty() {
            out.push_str(&String::from_utf16_lossy(pending));
            pending.clear();
        }
    };
    for &unit in units {
        if unit == 9 {
            flush(&mut pending, &mut out);
            let cells = cells_of(9, visual_column, tab_size);
            out.extend(std::iter::repeat_n(' ', cells as usize));
            visual_column += cells;
        } else {
            pending.push(unit);
            if !(0xDC00..0xE000).contains(&unit) {
                visual_column += cells_of(unit, visual_column, tab_size);
            }
        }
    }
    flush(&mut pending, &mut out);
    (out, visual_column)
}

/// O classificador de palavras do editor (`editor.wordSeparators` padrão, sem segmentação por locale).
pub fn word_classifier() -> Rc<WordCharacterClassifier> {
    let separators: Vec<u16> = USUAL_WORD_SEPARATORS.encode_utf16().collect();
    get_map_for_word_separators(&separators, &[])
}

pub fn class_of(classifier: &WordCharacterClassifier, unit: u16) -> WordCharacterClass {
    classifier.get(i32::from(unit))
}

/// Um trecho de caracteres da mesma classe, em posições de unidade (de `start` incluso a `end` excluso).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub start: usize,
    pub end: usize,
    pub class: WordCharacterClass,
}

/// Os trechos da linha por classe, com os espaços incluídos.
pub fn runs(classifier: &WordCharacterClassifier, line: &[u16]) -> Vec<Run> {
    let mut result: Vec<Run> = Vec::new();
    for (i, &unit) in line.iter().enumerate() {
        let class = class_of(classifier, unit);
        match result.last_mut() {
            Some(last) if last.class == class => last.end = i + 1,
            _ => result.push(Run { start: i, end: i + 1, class }),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn tabs_expand_to_stop() {
        assert_eq!(expand_tabs(&u("\tx"), 0, 4), ("    x".to_owned(), 5));
        assert_eq!(expand_tabs(&u("ab\tx"), 0, 4), ("ab  x".to_owned(), 5));
        assert_eq!(expand_tabs(&u("\t"), 2, 4), ("  ".to_owned(), 4));
    }

    #[test]
    fn ascii_and_cells() {
        assert!(is_basic_ascii(&u("fn main() {}")));
        assert!(!is_basic_ascii(&u("ação")));
        assert_eq!(expand_tabs(&u("日本"), 0, 4).1, 4);
    }

    #[test]
    fn runs_by_class() {
        let c = word_classifier();
        let r = runs(&c, &u("foo.bar  baz"));
        let spans: Vec<(usize, usize)> = r.iter().map(|r| (r.start, r.end)).collect();
        assert_eq!(spans, vec![(0, 3), (3, 4), (4, 7), (7, 9), (9, 12)]);
        assert_eq!(r[3].class, WordCharacterClass::Whitespace);
        assert_eq!(r[1].class, WordCharacterClass::WordSeparator);
    }
}
