//! Porte de `src/vs/base/common/filters.ts`: os filtros de correspondência do VS Code (prefixo, subtexto,
//! camel case, palavras, fuzzy) e o `fuzzyScore` do IntelliSense, do quick open e da paleta de comandos.
//!
//! Texto é `&[u16]` (unidades UTF-16): as posições devolvidas ([`Match`], [`FuzzyScore`]) são índices
//! UTF-16, que é como o editor mede o texto. Um filtro devolve `None` quando não casa (o `null` do original)
//! e `Some` com a lista de trechos quando casa, inclusive `Some(vec![])` (o `[]`, que é verdadeiro em
//! JavaScript).
//!
//! Diferenças do original:
//! - `or(...filter)` recebe um `Vec` de filtros e devolve o filtro combinado.
//! - `matchesPrefix` e `matchesStrictPrefix` (que no original são `_matchesPrefix.bind(undefined, ...)`)
//!   são funções comuns.
//! - `_matchesSubString` é um laço (a recursão do original só anda em linha reta: avança nas duas posições
//!   quando os caracteres são iguais e só na do alvo quando não são, o que é o casamento guloso de uma
//!   subsequência), para não estourar a pilha com alvos longos.
//! - O `alternateCharsCache` do `getAlternateCodes` não existe: [`crate::natural_language::korean`] só
//!   aloca quando a unidade é Hangul, então consultar é mais barato que um cache.
//! - As tabelas do `fuzzyScore` (`_table`, `_diag`, `_arrows`, `_minWordMatchPos`, `_maxWordMatchPos`) são
//!   globais no original e aqui são estado por thread, reaproveitado entre chamadas como lá.
//! - `typeof word !== 'string'` do `matchesFuzzy` não existe: os tipos garantem.
//! - `FuzzyScore` e `FuzzyScoreOptions` perdem o namespace e a classe abstrata: [`FUZZY_SCORE_DEFAULT`],
//!   [`is_default_fuzzy_score`] e [`FuzzyScoreOptions`] com [`FuzzyScoreOptions::DEFAULT`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use zyarr::RegExp;

use crate::JsString;
use crate::char_code as cc;
use crate::js_string::{self, char_code_at, code_point_at, index_of};
use crate::map::LRUCache;
use crate::natural_language::korean::get_korean_alt_chars;
use crate::normalization::try_normalize_to_base;
use crate::strings;

/// `IMatch`: um trecho `[start, end)` do texto, em unidades UTF-16.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub start: usize,
    pub end: usize,
}

/// `IFilter`: `None` se a palavra não casa com o texto.
pub type FilterFn = dyn Fn(&[u16], &[u16]) -> Option<Vec<Match>>;

/// Um filtro que é uma função comum (e não um fecho).
type PlainFilter = fn(&[u16], &[u16]) -> Option<Vec<Match>>;

// Combined filters

/// O filtro que combina o conjunto com um "ou": o *primeiro* que casa define o retorno.
pub fn or(filters: Vec<Box<FilterFn>>) -> impl Fn(&[u16], &[u16]) -> Option<Vec<Match>> {
    move |word, word_to_match_against| filters.iter().find_map(|filter| filter(word, word_to_match_against))
}

// Prefix

/// `matchesStrictPrefix`: o prefixo respeitando a caixa.
pub fn matches_strict_prefix(word: &[u16], word_to_match_against: &[u16]) -> Option<Vec<Match>> {
    matches_prefix_impl(false, word, word_to_match_against)
}

/// `matchesPrefix`: o prefixo ignorando a caixa.
pub fn matches_prefix(word: &[u16], word_to_match_against: &[u16]) -> Option<Vec<Match>> {
    matches_prefix_impl(true, word, word_to_match_against)
}

fn matches_prefix_impl(ignore_case: bool, word: &[u16], word_to_match_against: &[u16]) -> Option<Vec<Match>> {
    if word_to_match_against.is_empty() || word_to_match_against.len() < word.len() {
        return None;
    }

    let matches = if ignore_case {
        strings::starts_with_ignore_case(word_to_match_against, word)
    } else {
        index_of(word_to_match_against, word, 0) == Some(0)
    };

    if !matches {
        return None;
    }

    Some(if word.is_empty() { Vec::new() } else { vec![Match { start: 0, end: word.len() }] })
}

// Contiguous Substring

/// `matchesContiguousSubString`.
pub fn matches_contiguous_sub_string(word: &[u16], word_to_match_against: &[u16]) -> Option<Vec<Match>> {
    if word.len() > word_to_match_against.len() {
        return None;
    }

    let index = index_of(JsString::from(word_to_match_against).to_lower_case().units(), JsString::from(word).to_lower_case().units(), 0)?;

    Some(vec![Match { start: index, end: index + word.len() }])
}

/// `matchesBaseContiguousSubString`.
pub fn matches_base_contiguous_sub_string(word: &[u16], word_to_match_against: &[u16]) -> Option<Vec<Match>> {
    if word.len() > word_to_match_against.len() {
        return None;
    }

    let word = try_normalize_to_base(word);
    let word_to_match_against = try_normalize_to_base(word_to_match_against);
    let index = index_of(word_to_match_against.units(), word.units(), 0)?;

    Some(vec![Match { start: index, end: index + word.len() }])
}

// Substring

/// `matchesSubString`.
pub fn matches_sub_string(word: &[u16], word_to_match_against: &[u16]) -> Option<Vec<Match>> {
    if word.len() > word_to_match_against.len() {
        return None;
    }

    let word = JsString::from(word).to_lower_case();
    let target = JsString::from(word_to_match_against).to_lower_case();
    let (word, target) = (word.units(), target.units());

    // Casamento guloso de subsequência: os índices do alvo em que cada letra da palavra casou.
    let mut matched = Vec::with_capacity(word.len());
    let mut i = 0;
    for (j, &unit) in target.iter().enumerate() {
        if i == word.len() {
            break;
        }
        if word[i] == unit {
            matched.push(j);
            i += 1;
        }
    }
    if i < word.len() {
        return None;
    }

    // `join` do original, do fim para o começo: os índices consecutivos viram um trecho só.
    let mut result: Vec<Match> = Vec::new();
    for &j in &matched {
        match result.last_mut() {
            Some(last) if last.end == j => last.end = j + 1,
            _ => result.push(Match { start: j, end: j + 1 }),
        }
    }
    Some(result)
}

// CamelCase

fn is_lower(code: u16) -> bool {
    (cc::a..=cc::z).contains(&code)
}

/// `isUpper`.
pub fn is_upper(code: u16) -> bool {
    (cc::A..=cc::Z).contains(&code)
}

fn is_number(code: u16) -> bool {
    (cc::Digit0..=cc::Digit9).contains(&code)
}

fn is_whitespace(code: u16) -> bool {
    code == cc::Space || code == cc::Tab || code == cc::LineFeed || code == cc::CarriageReturn
}

/// Os separadores naturais de palavras do texto escrito, um subconjunto dos do monaco editor.
fn is_word_separator(code: Option<u16>) -> bool {
    code.is_some_and(|code| is_whitespace(code) || b"()[]{}<>`'\"-/;:,.?!".contains(&(code as u8)) && code < 0x80)
}

/// `charactersMatch`: `None` é o `NaN` de um `charCodeAt` fora do texto, que não casa com nada.
fn characters_match(code_a: Option<u16>, code_b: Option<u16>) -> bool {
    (code_a.is_some() && code_a == code_b) || (is_word_separator(code_a) && is_word_separator(code_b))
}

/// Códigos alternativos ao código passado: todos precisam casar *em ordem* para casar.
fn get_alternate_codes(code: u16) -> Option<Vec<u16>> {
    // Só há um provedor de códigos alternativos (o coreano); o tipo de retorno do original já leva isso em conta.
    get_korean_alt_chars(u32::from(code))
}

fn is_alphanumeric(code: u16) -> bool {
    is_lower(code) || is_upper(code) || is_number(code)
}

fn join(head: Match, mut tail: Vec<Match>) -> Vec<Match> {
    if tail.is_empty() {
        tail = vec![head];
    } else if head.end == tail[0].start {
        tail[0].start = head.start;
    } else {
        tail.insert(0, head);
    }
    tail
}

fn next_anchor(camel_case_word: &[u16], start: usize) -> usize {
    for i in start..camel_case_word.len() {
        let c = camel_case_word[i];
        if is_upper(c) || is_number(c) || (i > 0 && !is_alphanumeric(camel_case_word[i - 1])) {
            return i;
        }
    }
    camel_case_word.len()
}

/// `camelCaseWord[j].toLowerCase()` quando é uma unidade só (a única que pode igualar `word[i]`).
fn lower_unit(unit: u16) -> Option<u16> {
    if unit < 0x80 {
        return Some(if is_upper(unit) { unit + 32 } else { unit });
    }
    match JsString::from_units(vec![unit]).to_lower_case().units() {
        &[single] => Some(single),
        _ => None,
    }
}

fn matches_camel_case_at(word: &[u16], camel_case_word: &[u16], i: usize, j: usize) -> Option<Vec<Match>> {
    if i == word.len() {
        Some(Vec::new())
    } else if j == camel_case_word.len() || Some(word[i]) != lower_unit(camel_case_word[j]) {
        None
    } else {
        let mut next_upper_index = j + 1;
        let mut result = matches_camel_case_at(word, camel_case_word, i + 1, j + 1);
        while result.is_none() {
            next_upper_index = next_anchor(camel_case_word, next_upper_index);
            if next_upper_index >= camel_case_word.len() {
                break;
            }
            result = matches_camel_case_at(word, camel_case_word, i + 1, next_upper_index);
            next_upper_index += 1;
        }
        result.map(|result| join(Match { start: j, end: j + 1 }, result))
    }
}

struct CamelCaseAnalysis {
    upper_percent: f64,
    lower_percent: f64,
    alpha_percent: f64,
    numeric_percent: f64,
}

/// Heurística para não calcular o casador de camel case para palavras que não parecem camelCaseWords.
fn analyze_camel_case_word(word: &[u16]) -> CamelCaseAnalysis {
    let (mut upper, mut lower, mut alpha, mut numeric) = (0, 0, 0, 0);

    for &code in word {
        if is_upper(code) {
            upper += 1;
        }
        if is_lower(code) {
            lower += 1;
        }
        if is_alphanumeric(code) {
            alpha += 1;
        }
        if is_number(code) {
            numeric += 1;
        }
    }

    let len = word.len() as f64;
    CamelCaseAnalysis {
        upper_percent: f64::from(upper) / len,
        lower_percent: f64::from(lower) / len,
        alpha_percent: f64::from(alpha) / len,
        numeric_percent: f64::from(numeric) / len,
    }
}

fn is_upper_case_word(analysis: &CamelCaseAnalysis) -> bool {
    analysis.lower_percent == 0.0 && analysis.upper_percent > 0.6
}

fn is_camel_case_word(analysis: &CamelCaseAnalysis) -> bool {
    analysis.lower_percent > 0.2 && analysis.upper_percent < 0.8 && analysis.alpha_percent > 0.6 && analysis.numeric_percent < 0.2
}

/// Heurística para não calcular o casador de camel case para palavras que não parecem padrões de camel case.
fn is_camel_case_pattern(word: &[u16]) -> bool {
    let (mut upper, mut lower, mut whitespace) = (0, 0, 0);

    for &code in word {
        if is_upper(code) {
            upper += 1;
        }
        if is_lower(code) {
            lower += 1;
        }
        if is_whitespace(code) {
            whitespace += 1;
        }
    }

    if (upper == 0 || lower == 0) && whitespace == 0 { word.len() <= 30 } else { upper <= 5 }
}

/// `matchesCamelCase`.
pub fn matches_camel_case(word: &[u16], camel_case_word: &[u16]) -> Option<Vec<Match>> {
    if camel_case_word.is_empty() {
        return None;
    }

    let mut camel_case_word = js_string::trim(camel_case_word);

    if camel_case_word.is_empty() {
        return None;
    }

    if !is_camel_case_pattern(word) {
        return None;
    }

    // TODO do original: considerar remover esta checagem
    if camel_case_word.len() > 60 {
        camel_case_word = &camel_case_word[..60];
    }

    let analysis = analyze_camel_case_word(camel_case_word);

    let lowered;
    if !is_camel_case_word(&analysis) {
        if !is_upper_case_word(&analysis) {
            return None;
        }

        lowered = JsString::from(camel_case_word).to_lower_case();
        camel_case_word = lowered.units();
    }

    let word = JsString::from(word).to_lower_case();
    let mut i = 0;
    while i < camel_case_word.len() {
        if let Some(result) = matches_camel_case_at(word.units(), camel_case_word, 0, i) {
            return Some(result);
        }
        i = next_anchor(camel_case_word, i + 1);
    }

    None
}

// Matches beginning of words supporting non-ASCII languages
// If `contiguous` is true then matches word with beginnings of the words in the target. E.g. "pul" will match "Git: Pull"
// Otherwise also matches sub string of the word with beginnings of the words in the target. E.g. "gp" or "g p" will match "Git: Pull"
// Useful in cases where the target is words (e.g. command labels)

/// `matchesWords`.
pub fn matches_words(word: &[u16], target: &[u16], contiguous: bool) -> Option<Vec<Match>> {
    if target.is_empty() {
        return None;
    }

    let word = try_normalize_to_base(word);
    let target = try_normalize_to_base(target);
    // Memoiza as chamadas recursivas de uma invocação de nível superior. Como `characters_match` trata os
    // separadores de palavra como uma classe de equivalência, a recursão de `matches_words_at` pode, sem
    // isso, explodir exponencialmente para entradas como `editor.action` contra alvos com muitos separadores.
    let mut search = WordsSearch { word: word.units(), target: target.units(), contiguous, memo: HashMap::new() };

    let mut result = None;
    let mut target_index = 0;
    while target_index < target.len() {
        result = search.matches(0, target_index);
        if result.is_some() {
            break;
        }
        target_index = next_word(target.units(), target_index + 1);
    }

    result
}

struct WordsSearch<'a> {
    word: &'a [u16],
    target: &'a [u16],
    contiguous: bool,
    memo: HashMap<usize, Option<Vec<Match>>>,
}

impl WordsSearch<'_> {
    fn matches(&mut self, word_index: usize, target_index: usize) -> Option<Vec<Match>> {
        if word_index == self.word.len() {
            return Some(Vec::new());
        } else if target_index == self.target.len() {
            return None;
        }

        let memo_key = word_index * (self.target.len() + 1) + target_index;
        if let Some(cached) = self.memo.get(&memo_key) {
            // Quem chama (`join`) altera o vetor devolvido, então sempre devolve uma cópia.
            return cached.clone();
        }

        let computed = self.compute(word_index, target_index);
        self.memo.insert(memo_key, computed.clone());
        computed
    }

    fn compute(&mut self, word_index: usize, target_index: usize) -> Option<Vec<Match>> {
        let mut target_index_offset = 0;
        let word_code = char_code_at(self.word, word_index as i64);
        let target_code = |k: usize, target: &[u16]| char_code_at(target, (target_index + k) as i64);

        if !characters_match(word_code, target_code(0, self.target)) {
            // Confere os caracteres alternativos antes de desistir
            let alt_chars = get_alternate_codes(word_code?)?;
            for (k, &alt) in alt_chars.iter().enumerate() {
                if !characters_match(Some(alt), target_code(k, self.target)) {
                    return None;
                }
            }
            target_index_offset += alt_chars.len() - 1;
        }

        let mut next_word_index = target_index + target_index_offset + 1;
        let mut result = self.matches(word_index + 1, next_word_index);
        if !self.contiguous {
            while result.is_none() {
                next_word_index = next_word(self.target, next_word_index);
                if next_word_index >= self.target.len() {
                    break;
                }
                result = self.matches(word_index + 1, next_word_index);
                next_word_index += 1;
            }
        }

        let result = result?;

        // Se os caracteres não são exatamente iguais, então são separadores de palavra (ver
        // `characters_match`). Não entram nos trechos, mas o alvo não é descartado, então devolve `result`.
        if word_code != target_code(0, self.target) {
            // Confere os caracteres alternativos antes de sair
            let Some(alt_chars) = word_code.and_then(get_alternate_codes) else { return Some(result) };
            for (k, &alt) in alt_chars.iter().enumerate() {
                if Some(alt) != target_code(k, self.target) {
                    return Some(result);
                }
            }
        }

        Some(join(Match { start: target_index, end: target_index + target_index_offset + 1 }, result))
    }
}

fn next_word(word: &[u16], start: usize) -> usize {
    for i in start..word.len() {
        if is_word_separator(Some(word[i])) || (i > 0 && is_word_separator(Some(word[i - 1]))) {
            return i;
        }
    }
    word.len()
}

// Fuzzy

thread_local! {
    static FUZZY_REG_EXP_CACHE: LRUCache<JsString, Rc<RegExp>> = LRUCache::new(10_000, 1.0); // limitado a 10000 elementos
}

/// `matchesFuzzy`: primeiro uma `RegExp` com curinga (`*`), depois o filtro padrão (prefixo, camel case e
/// subtexto contíguo, ou separado quando `enable_separate_substring_matching`).
pub fn matches_fuzzy(word: &[u16], word_to_match_against: &[u16], enable_separate_substring_matching: bool) -> Option<Vec<Match>> {
    // Forma a RegExp das correspondências com curinga
    let key = JsString::from(word);
    let regexp = FUZZY_REG_EXP_CACHE.with(|cache| cache.get(&key)).unwrap_or_else(|| {
        let pattern = strings::convert_simple2_reg_exp_pattern(word);
        let regexp = Rc::new(RegExp::new(pattern.units(), &[u16::from(b'i')]).expect("o padrão simples é sempre uma regex válida"));
        FUZZY_REG_EXP_CACHE.with(|cache| {
            cache.set(key, regexp.clone());
        });
        regexp
    });

    // Filtro de RegExp
    if let Some(found) = regexp.exec(word_to_match_against, 0) {
        return Some(vec![Match { start: found.start(), end: found.end() }]);
    }

    // Filtro padrão
    let default_filter: [PlainFilter; 3] = [
        matches_prefix,
        matches_camel_case,
        if enable_separate_substring_matching { matches_sub_string } else { matches_contiguous_sub_string },
    ];
    default_filter.iter().find_map(|filter| filter(word, word_to_match_against))
}

/// Casa o padrão com a palavra de forma fuzzy, como no IntelliSense, mais rápido e mais poderoso que
/// [`matches_fuzzy`].
pub fn matches_fuzzy2(pattern: &[u16], word: &[u16]) -> Option<Vec<Match>> {
    let pattern_low = JsString::from(pattern).to_lower_case();
    let word_low = JsString::from(word).to_lower_case();
    let score = fuzzy_score(
        pattern,
        pattern_low.units(),
        0,
        word,
        word_low.units(),
        0,
        Some(&FuzzyScoreOptions { first_match_can_be_weak: true, boost_full_match: true }),
    );
    score.map(|score| create_matches(Some(&score)))
}

/// `anyScore`.
pub fn any_score(pattern: &[u16], low_pattern: &[u16], pattern_pos: usize, word: &[u16], low_word: &[u16], word_pos: usize) -> FuzzyScore {
    let max = pattern.len().min(13);
    for pattern_pos in pattern_pos..max {
        let result = fuzzy_score(
            pattern,
            low_pattern,
            pattern_pos,
            word,
            low_word,
            word_pos,
            Some(&FuzzyScoreOptions { first_match_can_be_weak: true, boost_full_match: true }),
        );
        if let Some(result) = result {
            return result;
        }
    }
    vec![0, word_pos as i32]
}

//#region --- fuzzyScore ---

/// `createMatches`: os trechos de um [`FuzzyScore`].
pub fn create_matches(score: Option<&[i32]>) -> Vec<Match> {
    let Some(score) = score else { return Vec::new() };
    let mut res: Vec<Match> = Vec::new();
    let word_pos = score[1];
    for i in (2..score.len()).rev() {
        let pos = (score[i] + word_pos) as usize;
        match res.last_mut() {
            Some(last) if last.end == pos => last.end = pos + 1,
            _ => res.push(Match { start: pos, end: pos + 1 }),
        }
    }
    res
}

const MAX_LEN: usize = 128;

/// As tabelas do `fuzzyScore`, globais no original.
struct Tables {
    /// Menor posição da palavra para cada posição do padrão.
    min_word_match_pos: Vec<usize>,
    /// Maior posição da palavra para cada posição do padrão.
    max_word_match_pos: Vec<usize>,
    /// O comprimento de uma diagonal contígua de casamento.
    diag: Vec<[i32; MAX_LEN + 1]>,
    table: Vec<[i32; MAX_LEN + 1]>,
    arrows: Vec<[Arrow; MAX_LEN + 1]>,
}

impl Tables {
    fn new() -> Self {
        Tables {
            min_word_match_pos: vec![0; 2 * MAX_LEN + 1],
            max_word_match_pos: vec![0; 2 * MAX_LEN + 1],
            diag: vec![[0; MAX_LEN + 1]; MAX_LEN + 1],
            table: vec![[0; MAX_LEN + 1]; MAX_LEN + 1],
            arrows: vec![[Arrow::Diag; MAX_LEN + 1]; MAX_LEN + 1],
        }
    }
}

thread_local! {
    static TABLES: RefCell<Tables> = RefCell::new(Tables::new());
}

/// `value[index]` do JavaScript: `None` é `undefined` (índice negativo ou fora do texto).
fn at(value: &[u16], index: i64) -> Option<u16> {
    char_code_at(value, index)
}

fn is_separator_at_pos(value: &[u16], index: i64) -> bool {
    if index < 0 || index >= value.len() as i64 {
        return false;
    }
    let Some(code) = code_point_at(value, index as usize) else { return false };
    match u16::try_from(code) {
        Ok(
            cc::Underline | cc::Dash | cc::Period | cc::Space | cc::Slash | cc::Backslash | cc::SingleQuote | cc::DoubleQuote
            | cc::Colon | cc::DollarSign | cc::LessThan | cc::GreaterThan | cc::OpenParen | cc::CloseParen
            | cc::OpenSquareBracket | cc::CloseSquareBracket | cc::OpenCurlyBrace | cc::CloseCurlyBrace,
        ) => true,
        _ => strings::is_emoji_imprecise(code),
    }
}

fn is_whitespace_at_pos(value: &[u16], index: i64) -> bool {
    if index < 0 || index >= value.len() as i64 {
        return false;
    }
    matches!(value[index as usize], cc::Space | cc::Tab)
}

fn is_upper_case_at_pos(pos: i64, word: &[u16], word_low: &[u16]) -> bool {
    at(word, pos) != at(word_low, pos)
}

/// `isPatternInWord`: os caracteres do padrão aparecem, em ordem, na palavra? Com `fill_min_word_pos_arr`
/// guarda a menor posição da palavra de cada posição do padrão.
pub fn is_pattern_in_word(
    pattern_low: &[u16],
    pattern_pos: usize,
    pattern_len: usize,
    word_low: &[u16],
    word_pos: usize,
    word_len: usize,
    fill_min_word_pos_arr: bool,
) -> bool {
    TABLES.with(|tables| is_pattern_in_word_with(&mut tables.borrow_mut(), pattern_low, pattern_pos, pattern_len, word_low, word_pos, word_len, fill_min_word_pos_arr))
}

#[allow(clippy::too_many_arguments)]
fn is_pattern_in_word_with(
    tables: &mut Tables,
    pattern_low: &[u16],
    mut pattern_pos: usize,
    pattern_len: usize,
    word_low: &[u16],
    mut word_pos: usize,
    word_len: usize,
    fill_min_word_pos_arr: bool,
) -> bool {
    while pattern_pos < pattern_len && word_pos < word_len {
        if at(pattern_low, pattern_pos as i64) == at(word_low, word_pos as i64) {
            if fill_min_word_pos_arr {
                // Lembra a menor posição da palavra para cada posição do padrão
                tables.min_word_match_pos[pattern_pos] = word_pos;
            }
            pattern_pos += 1;
        }
        word_pos += 1;
    }
    pattern_pos == pattern_len // o padrão precisa estar esgotado
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arrow {
    Diag = 1,
    Left = 2,
    LeftLeft = 3,
}

/// Um casamento fuzzy: `[score, wordStart, ...matches]`, em que `matches` são as posições, da última para a
/// primeira.
///
/// 0. a pontuação
/// 1. o deslocamento em que o casamento começou
/// 2. `<match_pos_N>`
/// 3. `<match_pos_1>`
/// 4. `<match_pos_0>` e assim por diante
pub type FuzzyScore = Vec<i32>;

/// `FuzzyScore.Default`: sem casamentos e valor `-100`.
pub const FUZZY_SCORE_DEFAULT: [i32; 2] = [-100, 0];

/// `FuzzyScore.isDefault`.
pub fn is_default_fuzzy_score(score: Option<&[i32]>) -> bool {
    score.is_none_or(|score| score == FUZZY_SCORE_DEFAULT)
}

/// `FuzzyScoreOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzyScoreOptions {
    pub first_match_can_be_weak: bool,
    pub boost_full_match: bool,
}

impl FuzzyScoreOptions {
    /// `FuzzyScoreOptions.default`.
    pub const DEFAULT: FuzzyScoreOptions = FuzzyScoreOptions { first_match_can_be_weak: false, boost_full_match: true };
}

impl Default for FuzzyScoreOptions {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `FuzzyScorer`.
pub type FuzzyScorerFn = dyn Fn(&[u16], &[u16], usize, &[u16], &[u16], usize, Option<&FuzzyScoreOptions>) -> Option<FuzzyScore>;

/// `fuzzyScore`.
pub fn fuzzy_score(
    pattern: &[u16],
    pattern_low: &[u16],
    pattern_start: usize,
    word: &[u16],
    word_low: &[u16],
    word_start: usize,
    options: Option<&FuzzyScoreOptions>,
) -> Option<FuzzyScore> {
    let options = options.copied().unwrap_or_default();
    TABLES.with(|tables| fuzzy_score_with(&mut tables.borrow_mut(), pattern, pattern_low, pattern_start, word, word_low, word_start, options))
}

#[allow(clippy::too_many_arguments)]
fn fuzzy_score_with(
    tables: &mut Tables,
    pattern: &[u16],
    pattern_low: &[u16],
    pattern_start: usize,
    word: &[u16],
    word_low: &[u16],
    word_start: usize,
    options: FuzzyScoreOptions,
) -> Option<FuzzyScore> {
    let pattern_len = pattern.len().min(MAX_LEN);
    let word_len = word.len().min(MAX_LEN);

    if pattern_start >= pattern_len || word_start >= word_len || (pattern_len - pattern_start) > (word_len - word_start) {
        return None;
    }

    // Uma checagem simples de que os caracteres do padrão ocorrem (em ordem) na palavra. Se não ocorrem,
    // para aqui, porque nenhum casamento é possível
    if !is_pattern_in_word_with(tables, pattern_low, pattern_start, pattern_len, word_low, word_start, word_len, true) {
        return None;
    }

    // Acha a maior posição da palavra para cada posição do padrão
    // NOTA: a menor posição foi preenchida acima, na chamada de `is_pattern_in_word`
    fill_in_max_word_match_pos(tables, pattern_len, word_len, pattern_start, word_start, pattern_low, word_low);

    let mut row;
    let mut column = 0usize;
    let mut has_strong_first_match = false;

    // Haverá um casamento, preenche as tabelas
    row = 1;
    let mut pattern_pos = pattern_start;
    while pattern_pos < pattern_len {
        // Reduz o espaço de busca às posições possíveis da palavra e ao acesso possível da próxima linha
        let min_word_match_pos = tables.min_word_match_pos[pattern_pos];
        let max_word_match_pos = tables.max_word_match_pos[pattern_pos];
        let next_max_word_match_pos = if pattern_pos + 1 < pattern_len { tables.max_word_match_pos[pattern_pos + 1] } else { word_len };

        column = min_word_match_pos - word_start + 1;
        let mut word_pos = min_word_match_pos;
        while word_pos < next_max_word_match_pos {
            let mut score = None;
            let mut can_come_diag = false;

            if word_pos <= max_word_match_pos {
                score = do_score(
                    pattern,
                    pattern_low,
                    pattern_pos,
                    pattern_start,
                    word,
                    word_low,
                    word_pos,
                    word_len,
                    word_start,
                    tables.diag[row - 1][column - 1] == 0,
                    &mut has_strong_first_match,
                );
            }

            let mut diag_score = 0;
            if let Some(score) = score {
                can_come_diag = true;
                diag_score = score + tables.table[row - 1][column - 1];
            }

            let can_come_left = word_pos > min_word_match_pos;
            let left_score = if can_come_left {
                tables.table[row][column - 1] + if tables.diag[row][column - 1] > 0 { -5 } else { 0 } // penalidade pelo começo de uma lacuna
            } else {
                0
            };

            let can_come_left_left = word_pos > min_word_match_pos + 1 && tables.diag[row][column - 1] > 0;
            let left_left_score = if can_come_left_left {
                tables.table[row][column - 2] + if tables.diag[row][column - 2] > 0 { -5 } else { 0 } // penalidade pelo começo de uma lacuna
            } else {
                0
            };

            if can_come_left_left && (!can_come_left || left_left_score >= left_score) && (!can_come_diag || left_left_score >= diag_score) {
                // sempre prefere o left left para pular uma diagonal, porque isso põe o casamento mais cedo na palavra
                tables.table[row][column] = left_left_score;
                tables.arrows[row][column] = Arrow::LeftLeft;
                tables.diag[row][column] = 0;
            } else if can_come_left && (!can_come_diag || left_score >= diag_score) {
                // sempre prefere o left, porque isso põe o casamento mais cedo na palavra
                tables.table[row][column] = left_score;
                tables.arrows[row][column] = Arrow::Left;
                tables.diag[row][column] = 0;
            } else if can_come_diag {
                tables.table[row][column] = diag_score;
                tables.arrows[row][column] = Arrow::Diag;
                tables.diag[row][column] = tables.diag[row - 1][column - 1] + 1;
            } else {
                panic!("not possible");
            }

            word_pos += 1;
            column += 1;
        }

        pattern_pos += 1;
        row += 1;
    }

    if !has_strong_first_match && !options.first_match_can_be_weak {
        return None;
    }

    row -= 1;
    column -= 1;

    let mut result: FuzzyScore = vec![tables.table[row][column], word_start as i32];

    let mut backwards_diag_length = 0;
    let mut max_match_column = 0i64;

    // `row` e `column` são índices de tabela; `diag_column` pode ir a zero ou abaixo (o `do/while` do
    // original sai quando `diagColumn < 1`), então as colunas são `i64`.
    let mut column = column as i64;
    while row >= 1 {
        // Acha a coluna em que se sobe na diagonal
        let mut diag_column = column;
        loop {
            match tables.arrows[row][diag_column as usize] {
                Arrow::LeftLeft => diag_column -= 2,
                Arrow::Left => diag_column -= 1,
                // achou a diagonal
                Arrow::Diag => break,
            }
            if diag_column < 1 {
                break;
            }
        }

        // Anula a decisão "para frente" se manter a diagonal "para trás" daria um casamento melhor
        if backwards_diag_length > 1 // só se houver um casamento contíguo de 3 caracteres
            && at(pattern_low, (pattern_start + row) as i64 - 1) == at(word_low, word_start as i64 + column - 1) // só se der para fazer um casamento contíguo na diagonal
            && !is_upper_case_at_pos(diag_column + word_start as i64 - 1, word, word_low) // só se o diagonal escolhido "para frente" não for uma maiúscula
            && backwards_diag_length + 1 > tables.diag[row][diag_column as usize] // só se o nosso casamento contíguo fosse mais longo que o "para frente"
        {
            diag_column = column;
        }

        if diag_column == column {
            // este é um casamento contíguo
            backwards_diag_length += 1;
        } else {
            backwards_diag_length = 1;
        }

        if max_match_column == 0 {
            // lembra a última coluna casada
            max_match_column = diag_column;
        }

        row -= 1;
        column = diag_column - 1;
        result.push(column as i32);
    }

    if (word_len - word_start) == pattern_len && options.boost_full_match {
        // a palavra casa com todos os caracteres do padrão!
        // dá à pontuação um bônus de casamento total (para passar à frente das outras palavras)
        result[0] += 2;
    }

    // Soma 1 de penalidade por cada caractere pulado na palavra
    let skipped_chars_count = max_match_column - pattern_len as i64;
    result[0] -= skipped_chars_count as i32;

    Some(result)
}

fn fill_in_max_word_match_pos(
    tables: &mut Tables,
    pattern_len: usize,
    word_len: usize,
    pattern_start: usize,
    word_start: usize,
    pattern_low: &[u16],
    word_low: &[u16],
) {
    let mut pattern_pos = pattern_len as i64 - 1;
    let mut word_pos = word_len as i64 - 1;
    while pattern_pos >= pattern_start as i64 && word_pos >= word_start as i64 {
        if at(pattern_low, pattern_pos) == at(word_low, word_pos) {
            tables.max_word_match_pos[pattern_pos as usize] = word_pos as usize;
            pattern_pos -= 1;
        }
        word_pos -= 1;
    }
}

/// `_doScore`: `None` é o `Number.MIN_SAFE_INTEGER` do original (sem casamento de caractere).
#[allow(clippy::too_many_arguments)]
fn do_score(
    pattern: &[u16],
    pattern_low: &[u16],
    pattern_pos: usize,
    pattern_start: usize,
    word: &[u16],
    word_low: &[u16],
    word_pos: usize,
    word_len: usize,
    word_start: usize,
    new_match_start: bool,
    out_first_match_strong: &mut bool,
) -> Option<i32> {
    if at(pattern_low, pattern_pos as i64) != at(word_low, word_pos as i64) {
        return None;
    }

    let (pos, previous) = (word_pos as i64, word_pos as i64 - 1);
    let mut score = 1;
    let mut is_gap_location = false;
    if word_pos == pattern_pos - pattern_start {
        // prefixo comum: `foobar <-> foobaz`
        //                            ^^^^^
        score = if at(pattern, pattern_pos as i64) == at(word, pos) { 7 } else { 5 };
    } else if is_upper_case_at_pos(pos, word, word_low) && (word_pos == 0 || !is_upper_case_at_pos(previous, word, word_low)) {
        // maiúscula: `foo <-> forOthers`
        //                      ^^ ^
        score = if at(pattern, pattern_pos as i64) == at(word, pos) { 7 } else { 5 };
        is_gap_location = true;
    } else if is_separator_at_pos(word_low, pos) && (word_pos == 0 || !is_separator_at_pos(word_low, previous)) {
        // um separador: `. <-> foo.bar`
        //                        ^
        score = 5;
    } else if is_separator_at_pos(word_low, previous) || is_whitespace_at_pos(word_low, previous) {
        // depois de um separador: `foo <-> bar_foo`
        //                                      ^^^
        score = 5;
        is_gap_location = true;
    }

    if score > 1 && pattern_pos == pattern_start {
        *out_first_match_strong = true;
    }

    if !is_gap_location {
        is_gap_location = is_upper_case_at_pos(pos, word, word_low) || is_separator_at_pos(word_low, previous) || is_whitespace_at_pos(word_low, previous);
    }

    if pattern_pos == pattern_start {
        // primeiro caractere do padrão
        if word_pos > word_start {
            // o primeiro caractere do padrão casaria com um caractere da palavra que não é o começo dela,
            // então põe uma penalidade pela lacuna que precede este casamento
            score -= if is_gap_location { 3 } else { 5 };
        }
    } else if new_match_start {
        // este seria o começo de um novo casamento (isto é, haveria uma lacuna antes deste ponto)
        score += if is_gap_location { 2 } else { 0 };
    } else {
        // isto é parte de um casamento contíguo, então dá um pequeno bônus, mas só se não for um local
        // de lacuna preferido
        score += if is_gap_location { 0 } else { 1 };
    }

    if word_pos + 1 == word_len {
        // sempre penaliza lacunas, mas isso dá uma vantagem injusta a um casamento que pegaria o último
        // caractere da palavra, então finge que há uma lacuna depois do último caractere para normalizar
        score -= if is_gap_location { 3 } else { 5 };
    }

    Some(score)
}

//#endregion

//#region --- graceful ---

/// `fuzzyScoreGracefulAggressive`.
pub fn fuzzy_score_graceful_aggressive(
    pattern: &[u16],
    low_pattern: &[u16],
    pattern_pos: usize,
    word: &[u16],
    low_word: &[u16],
    word_pos: usize,
    options: Option<&FuzzyScoreOptions>,
) -> Option<FuzzyScore> {
    fuzzy_score_with_permutations(pattern, low_pattern, pattern_pos, word, low_word, word_pos, true, options)
}

/// `fuzzyScoreGraceful`.
pub fn fuzzy_score_graceful(
    pattern: &[u16],
    low_pattern: &[u16],
    pattern_pos: usize,
    word: &[u16],
    low_word: &[u16],
    word_pos: usize,
    options: Option<&FuzzyScoreOptions>,
) -> Option<FuzzyScore> {
    fuzzy_score_with_permutations(pattern, low_pattern, pattern_pos, word, low_word, word_pos, false, options)
}

#[allow(clippy::too_many_arguments)]
fn fuzzy_score_with_permutations(
    pattern: &[u16],
    low_pattern: &[u16],
    pattern_pos: usize,
    word: &[u16],
    low_word: &[u16],
    word_pos: usize,
    aggressive: bool,
    options: Option<&FuzzyScoreOptions>,
) -> Option<FuzzyScore> {
    let mut top = fuzzy_score(pattern, low_pattern, pattern_pos, word, low_word, word_pos, options);

    if top.is_some() && !aggressive {
        // quando o padrão original dá um resultado, ele é devolvido, a não ser que sejamos agressivos e
        // tentemos achar um alinhamento melhor, por exemplo `cno` -> `^co^ns^ole` ou `^c^o^nsole`.
        return top;
    }

    if pattern.len() >= 3 {
        // Quando o padrão é longo o bastante, tenta algumas permutações (no máximo 7) dele. As permutações
        // só trocam caracteres vizinhos, por exemplo `cnoso` vira `conso`, `cnsoo`, `cnoos`.
        let tries = (pattern.len() - 1).min(7);
        for moving_pattern_pos in (pattern_pos + 1)..tries {
            if let Some(new_pattern) = next_typo_permutation(pattern, moving_pattern_pos) {
                let new_pattern_low = JsString::from(new_pattern.as_slice()).to_lower_case();
                let candidate = fuzzy_score(&new_pattern, new_pattern_low.units(), pattern_pos, word, low_word, word_pos, options);
                if let Some(mut candidate) = candidate {
                    candidate[0] -= 3; // penalidade da permutação
                    if top.as_ref().is_none_or(|top| candidate[0] > top[0]) {
                        top = Some(candidate);
                    }
                }
            }
        }
    }

    top
}

fn next_typo_permutation(pattern: &[u16], pattern_pos: usize) -> Option<Vec<u16>> {
    if pattern_pos + 1 >= pattern.len() {
        return None;
    }

    let swap1 = pattern[pattern_pos];
    let swap2 = pattern[pattern_pos + 1];

    if swap1 == swap2 {
        return None;
    }

    let mut result = pattern.to_vec();
    result.swap(pattern_pos, pattern_pos + 1);
    Some(result)
}

//#endregion
