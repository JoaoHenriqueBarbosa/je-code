//! Porte de `src/vs/editor/common/services/unicodeTextModelHighlighter.ts`.
//!
//! Texto segue a semântica do JavaScript: colunas e deslocamentos contam unidades UTF-16.
//!
//! Notas de tradução:
//! - `IUnicodeHighlightsResult` (de `editorWorker.ts`) mora aqui, com os campos que a função preenche.
//! - `UnicodeHighlighterReasonKind` (const enum) é a própria variante de [`UnicodeHighlighterReason`].
//! - `IUnicodeCharacterSearcherTarget` vira o trait [`UnicodeCharacterSearcherTarget`], implementado para
//!   o [`TextModel`].
//! - O `Set` de candidatos do original mantém a ordem de inserção; aqui é um `BTreeSet`, e a ordem não
//!   muda o que a classe de caracteres do regex casa.

use crate::cursor_common::CursorSimpleModel;
use std::collections::BTreeSet;

use je_base::JsString;
use je_base::assert::assert_never;
use je_base::js_string::{char_code_at, code_point_at, substring};
use je_base::strings::{self, AmbiguousCharacters, InvisibleCharacters};

use je_base::js_regexp::JsRegExp;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::text_model_search::Searcher;
use crate::word_helper::{default_word_regexp, get_word_at_text};

/// `IUnicodeHighlightsResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnicodeHighlightsResult {
    pub ranges: Vec<Range>,
    pub has_more: bool,
    pub non_basic_ascii_character_count: usize,
    pub invisible_character_count: usize,
    pub ambiguous_character_count: usize,
}

/// `IUnicodeCharacterSearcherTarget`.
pub trait UnicodeCharacterSearcherTarget {
    fn get_line_count(&self) -> i32;
    fn get_line_content(&self, line_number: i32) -> JsString;
}

impl UnicodeCharacterSearcherTarget for TextModel {
    fn get_line_count(&self) -> i32 {
        <TextModel as CursorSimpleModel>::get_line_count(self)
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        <TextModel as CursorSimpleModel>::get_line_content(self, line_number)
    }
}

/// `UnicodeHighlighterOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnicodeHighlighterOptions {
    pub non_basic_ascii: bool,
    pub ambiguous_characters: bool,
    pub invisible_characters: bool,
    pub include_comments: bool,
    pub include_strings: bool,
    pub allowed_code_points: Vec<u32>,
    pub allowed_locales: Vec<String>,
}

/// `UnicodeHighlighterReason`; o `kind` do original é a variante.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnicodeHighlighterReason {
    Ambiguous {
        confusable_with: JsString,
        not_ambiguous_in_locales: Vec<String>,
    },
    Invisible,
    NonBasicAscii,
}

/// `SimpleHighlightReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SimpleHighlightReason {
    None,
    NonBasicAscii,
    Invisible,
    Ambiguous,
}

/// Os candidatos de `getCandidateCodePoints`: todos os não ASCII básicos ou um conjunto.
enum CandidateCodePoints {
    AllNonBasicAscii,
    Set(BTreeSet<u32>),
}

/// `String.fromCodePoint(codePoint)` em unidades UTF-16.
fn from_code_point(code_point: u32) -> Vec<u16> {
    if code_point <= 0xFFFF {
        return vec![code_point as u16];
    }
    let offset = code_point - 0x10000;
    vec![0xD800 + (offset >> 10) as u16, 0xDC00 + (offset & 0x3FF) as u16]
}

/// `new Set([...values])`: tira as repetições e mantém a ordem da primeira ocorrência.
fn dedup_in_order(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

/// `isAllowedInvisibleCharacter`.
fn is_allowed_invisible_character(character: &[u16]) -> bool {
    matches!(character, [0x20] | [0x0A] | [0x09])
}

/// `buildRegExpCharClassExpr`.
fn build_reg_exp_char_class_expr(code_points: &[u32]) -> Vec<u16> {
    let joined: Vec<u16> = code_points.iter().flat_map(|&cp| from_code_point(cp)).collect();
    let mut src: Vec<u16> = vec![u16::from(b'[')];
    src.extend_from_slice(strings::escape_reg_exp_characters(&joined).units());
    src.push(u16::from(b']'));
    src
}

/// `CodePointHighlighter`.
struct CodePointHighlighter<'a> {
    options: &'a UnicodeHighlighterOptions,
    allowed_code_points: BTreeSet<u32>,
    ambiguous_characters: std::sync::Arc<AmbiguousCharacters>,
}

impl<'a> CodePointHighlighter<'a> {
    fn new(options: &'a UnicodeHighlighterOptions) -> Self {
        Self {
            options,
            allowed_code_points: options.allowed_code_points.iter().copied().collect(),
            ambiguous_characters: AmbiguousCharacters::get_instance(dedup_in_order(options.allowed_locales.iter().cloned())),
        }
    }

    fn get_candidate_code_points(&self) -> CandidateCodePoints {
        if self.options.non_basic_ascii {
            return CandidateCodePoints::AllNonBasicAscii;
        }

        let mut set = BTreeSet::new();

        if self.options.invisible_characters {
            for &cp in InvisibleCharacters::code_points() {
                if !is_allowed_invisible_character(&from_code_point(cp)) {
                    set.insert(cp);
                }
            }
        }

        if self.options.ambiguous_characters {
            set.extend(self.ambiguous_characters.get_confusable_code_points());
        }

        for cp in &self.allowed_code_points {
            set.remove(cp);
        }

        CandidateCodePoints::Set(set)
    }

    fn should_highlight_non_basic_ascii(&self, character: &[u16], word_context: Option<&[u16]>) -> SimpleHighlightReason {
        let code_point = code_point_at(character, 0).expect("o caractere não pode ser vazio");

        if self.allowed_code_points.contains(&code_point) {
            return SimpleHighlightReason::None;
        }

        if self.options.non_basic_ascii {
            return SimpleHighlightReason::NonBasicAscii;
        }

        let mut has_basic_ascii_characters = false;
        let mut has_non_confusable_non_basic_ascii_character = false;
        if let Some(word_context) = word_context {
            // `for (const char of wordContext)`: itera por code points.
            let mut index = 0;
            while index < word_context.len() {
                let cp = code_point_at(word_context, index).expect("o índice está dentro do texto");
                let width = if cp > 0xFFFF { 2 } else { 1 };
                let is_basic_ascii = strings::is_basic_ascii(&word_context[index..index + width]);
                has_basic_ascii_characters = has_basic_ascii_characters || is_basic_ascii;

                if !is_basic_ascii && !self.ambiguous_characters.is_ambiguous(cp) && !InvisibleCharacters::is_invisible_character(cp) {
                    has_non_confusable_non_basic_ascii_character = true;
                }
                index += width;
            }
        }

        if
        /* Don't allow mixing weird looking characters with ASCII */
        !has_basic_ascii_characters
            /* Is there an obviously weird looking character? */
            && has_non_confusable_non_basic_ascii_character
        {
            return SimpleHighlightReason::None;
        }

        if self.options.invisible_characters {
            // TODO check for emojis
            if !is_allowed_invisible_character(character) && InvisibleCharacters::is_invisible_character(code_point) {
                return SimpleHighlightReason::Invisible;
            }
        }

        if self.options.ambiguous_characters && self.ambiguous_characters.is_ambiguous(code_point) {
            return SimpleHighlightReason::Ambiguous;
        }

        SimpleHighlightReason::None
    }
}

/// `UnicodeTextModelHighlighter`.
pub struct UnicodeTextModelHighlighter;

impl UnicodeTextModelHighlighter {
    pub fn compute_unicode_highlights(
        model: &impl UnicodeCharacterSearcherTarget,
        options: &UnicodeHighlighterOptions,
        range: Option<Range>,
    ) -> UnicodeHighlightsResult {
        let start_line = range.map_or(1, |r| r.start_line_number);
        let end_line = range.map_or_else(|| model.get_line_count(), |r| r.end_line_number);

        let code_point_highlighter = CodePointHighlighter::new(options);

        let regex = match code_point_highlighter.get_candidate_code_points() {
            CandidateCodePoints::AllNonBasicAscii => JsRegExp::from_str("[^\\t\\n\\r\\x20-\\x7E]", "g"),
            CandidateCodePoints::Set(candidates) => {
                let candidates: Vec<u32> = candidates.into_iter().collect();
                JsRegExp::new(&build_reg_exp_char_class_expr(&candidates), "g")
            }
        }
        .expect("o padrão gerado para os candidatos é válido");

        let mut searcher = Searcher::new(None, &regex);
        let word_regexp = default_word_regexp();
        let mut ranges: Vec<Range> = Vec::new();
        let mut has_more = false;

        let mut ambiguous_character_count = 0;
        let mut invisible_character_count = 0;
        let mut non_basic_ascii_character_count = 0;

        'for_loop: for line_number in start_line..=end_line {
            let line_content = model.get_line_content(line_number);
            let line = line_content.units();
            let line_length = line.len();

            // Reset regex to search from the beginning
            searcher.reset(0);
            while let Some(m) = searcher.next(line) {
                let mut start_index = m.start();
                let mut end_index = m.end();

                // Extend range to entire code point
                if start_index > 0
                    && let Some(char_code_before) = char_code_at(line, start_index as i64 - 1)
                    && strings::is_high_surrogate(char_code_before)
                {
                    start_index -= 1;
                }
                if end_index + 1 < line_length
                    && let Some(char_code_before) = char_code_at(line, end_index as i64 - 1)
                    && strings::is_high_surrogate(char_code_before)
                {
                    end_index += 1;
                }
                let str = substring(line, start_index as i64, end_index as i64);
                let mut word = get_word_at_text(start_index as i32 + 1, &word_regexp, line, 0, None);
                if word.as_ref().is_some_and(|w| w.end_column <= start_index as i32 + 1) {
                    // The word does not include the problematic character, ignore the word
                    word = None;
                }
                let highlight_reason =
                    code_point_highlighter.should_highlight_non_basic_ascii(str, word.as_ref().map(|w| w.word.units()));

                if highlight_reason != SimpleHighlightReason::None {
                    match highlight_reason {
                        SimpleHighlightReason::Ambiguous => ambiguous_character_count += 1,
                        SimpleHighlightReason::Invisible => invisible_character_count += 1,
                        SimpleHighlightReason::NonBasicAscii => non_basic_ascii_character_count += 1,
                        SimpleHighlightReason::None => assert_never(None),
                    }

                    const MAX_RESULT_LENGTH: usize = 1000;
                    if ranges.len() >= MAX_RESULT_LENGTH {
                        has_more = true;
                        break 'for_loop;
                    }

                    ranges.push(Range::new(line_number, start_index as i32 + 1, line_number, end_index as i32 + 1));
                }
            }
        }

        UnicodeHighlightsResult {
            ranges,
            has_more,
            ambiguous_character_count,
            invisible_character_count,
            non_basic_ascii_character_count,
        }
    }

    pub fn compute_unicode_highlight_reason(char: &[u16], options: &UnicodeHighlighterOptions) -> Option<UnicodeHighlighterReason> {
        let code_point_highlighter = CodePointHighlighter::new(options);

        match code_point_highlighter.should_highlight_non_basic_ascii(char, None) {
            SimpleHighlightReason::None => None,
            SimpleHighlightReason::Invisible => Some(UnicodeHighlighterReason::Invisible),
            SimpleHighlightReason::Ambiguous => {
                let code_point = code_point_at(char, 0).expect("o caractere não pode ser vazio");
                let primary_confusable = code_point_highlighter
                    .ambiguous_characters
                    .get_primary_confusable(code_point)
                    .expect("um caractere ambíguo tem confundível primário");
                let not_ambiguous_in_locales = AmbiguousCharacters::get_locales()
                    .into_iter()
                    .filter(|l| {
                        let locales = dedup_in_order(
                            options.allowed_locales.iter().cloned().chain(std::iter::once((*l).to_owned())),
                        );
                        !AmbiguousCharacters::get_instance(locales).is_ambiguous(code_point)
                    })
                    .map(str::to_owned)
                    .collect();
                Some(UnicodeHighlighterReason::Ambiguous {
                    confusable_with: JsString::from_units(from_code_point(primary_confusable)),
                    not_ambiguous_in_locales,
                })
            }
            SimpleHighlightReason::NonBasicAscii => Some(UnicodeHighlighterReason::NonBasicAscii),
        }
    }
}
