//! Porte de `src/vs/editor/common/languages/supports/richEditBrackets.ts`: os colchetes de uma linguagem
//! agrupados (`RichEditBracket`, `RichEditBrackets`) com as regex de busca para frente e para trás, e
//! `BracketsUtils`.
//!
//! Decisões de tradução:
//! - Os colchetes são comparados com o texto do modelo, então viram [`JsString`] (UTF-16) aqui dentro;
//!   a configuração entra como [`CharacterPair`] de `String`.
//! - `createRegExp` e `RegExpOptions` (de `strings.ts`) ainda não existem no `je-base`: ficam aqui, com o
//!   mesmo comportamento, até o `strings.ts` ser completado.
//! - `toReversedString` do original guarda a última entrada em cache; aqui é uma função pura.
//! - `text.match(regex)` com um regex global devolve todos os casamentos e `m.index` fica indefinido
//!   (vira 0): [`match_first`] reproduz isso.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::JsString;
use je_base::errors::{Error, throw};
use je_base::js_string::{index_of, substring};
use je_base::strings::escape_reg_exp_characters;

use crate::js_reg_exp::JsRegExp;
use crate::language_configuration::CharacterPair;
use crate::range::Range;

/// `RegExpOptions`.
// TODO(integração): mover para `je_base::strings`, junto de `create_reg_exp`.
#[derive(Clone, Copy, Debug, Default)]
pub struct RegExpOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub multiline: bool,
    pub global: bool,
    pub unicode: bool,
}

/// `/\w/.test(c)` para um caractere: `[A-Za-z0-9_]`.
pub(crate) fn is_word_unit(unit: u16) -> bool {
    matches!(unit, 0x30..=0x39 | 0x41..=0x5a | 0x61..=0x7a | 0x5f)
}

/// `!/\B/.test(c)` para o `charAt` de um texto: vale para um caractere de palavra, e para a posição
/// vazia o `\B` casa.
pub(crate) fn is_word_boundary_char(unit: Option<u16>) -> bool {
    unit.is_some_and(is_word_unit)
}

/// `createRegExp(searchString, isRegex, options)`.
// TODO(integração): mover para `je_base::strings`.
pub fn create_reg_exp(search_string: &[u16], is_regex: bool, options: &RegExpOptions) -> JsRegExp {
    if search_string.is_empty() {
        throw(Error::new("Cannot create regex from empty string"));
    }
    let mut source: Vec<u16> = if is_regex {
        search_string.to_vec()
    } else {
        escape_reg_exp_characters(search_string).units().to_vec()
    };
    if options.whole_word {
        if is_word_boundary_char(source.first().copied()) {
            source.splice(0..0, "\\b".encode_utf16());
        }
        if is_word_boundary_char(source.last().copied()) {
            source.extend("\\b".encode_utf16());
        }
    }
    let mut modifiers = String::new();
    if options.global {
        modifiers.push('g');
    }
    if !options.match_case {
        modifiers.push('i');
    }
    if options.multiline {
        modifiers.push('m');
    }
    if options.unicode {
        modifiers.push('u');
    }
    JsRegExp::new(&source, &modifiers)
        .unwrap_or_else(|error| throw(Error::named("SyntaxError", error.message())))
}

/// `toReversedString`: inverte as unidades UTF-16 e passa pelo decodificador, que troca por U+FFFD os
/// substitutos que ficam fora de ordem.
fn to_reversed_string(text: &[u16]) -> JsString {
    let reversed: Vec<u16> = text.iter().rev().copied().collect();
    JsString::from(String::from_utf16_lossy(&reversed))
}

/// Um grupo de colchetes que colidem (`InternalBracket`).
struct InternalBracket {
    open: Vec<JsString>,
    close: Vec<JsString>,
}

/// `RichEditBracket`: um grupo de pares de colchetes que colidem.
///
/// Quase sempre tem um par só, mas o mesmo texto pode fechar vários pares, ou abrir vários, e então o
/// grupo tem vários, como `open: ['if', 'for'], close: ['end', 'end']`.
#[derive(Debug)]
pub struct RichEditBracket {
    pub language_id: String,
    /// Identificador único, consecutivo a partir de 0, deste grupo.
    pub index: usize,
    /// As aberturas de cada par; `open[i]` corresponde a `close[i]`.
    pub open: Vec<JsString>,
    /// Os fechamentos de cada par.
    pub close: Vec<JsString>,
    /// Regex para achar este grupo num texto, ciente dos outros colchetes (ver `getRegexForBracketPair`).
    pub forward_regex: Rc<JsRegExp>,
    /// O mesmo, para buscar de trás para frente (ver `getReversedRegexForBracketPair`).
    pub reversed_regex: Rc<JsRegExp>,
    open_set: HashSet<JsString>,
    close_set: HashSet<JsString>,
}

impl RichEditBracket {
    pub fn new(
        language_id: &str,
        index: usize,
        open: Vec<JsString>,
        close: Vec<JsString>,
        forward_regex: JsRegExp,
        reversed_regex: JsRegExp,
    ) -> Self {
        Self {
            language_id: language_id.to_owned(),
            index,
            open_set: open.iter().cloned().collect(),
            close_set: close.iter().cloned().collect(),
            open,
            close,
            forward_regex: Rc::new(forward_regex),
            reversed_regex: Rc::new(reversed_regex),
        }
    }

    /// `text` é uma abertura deste grupo?
    pub fn is_open(&self, text: &[u16]) -> bool {
        self.open_set.contains(text)
    }

    /// `text` é um fechamento deste grupo?
    pub fn is_close(&self, text: &[u16]) -> bool {
        self.close_set.contains(text)
    }
}

/// `groupFuzzyBrackets`: junta os colchetes que têm a mesma abertura ou o mesmo fechamento. Por exemplo
/// `['IF','END']`, `['for','end']` e `['{','}']` viram `{ open: ['if', 'for'], close: ['end', 'end'] }` e
/// `{ open: ['{'], close: ['}'] }`.
fn group_fuzzy_brackets(brackets: &[CharacterPair]) -> Vec<InternalBracket> {
    let n = brackets.len();
    let brackets: Vec<(JsString, JsString)> = brackets
        .iter()
        .map(|(open, close)| {
            (
                JsString::from(open.as_str()).to_lower_case(),
                JsString::from(close.as_str()).to_lower_case(),
            )
        })
        .collect();

    let mut group: Vec<usize> = (0..n).collect();

    let are_overlapping = |a: &(JsString, JsString), b: &(JsString, JsString)| {
        a.0 == b.0 || a.0 == b.1 || a.1 == b.0 || a.1 == b.1
    };

    // Junta os colchetes de mesma abertura ou mesmo fechamento.
    for i in 0..n {
        for j in i + 1..n {
            if are_overlapping(&brackets[i], &brackets[j]) {
                let (g1, g2) = (group[i], group[j]);
                let (new_g, old_g) = (g1.min(g2), g1.max(g2));
                for g in group.iter_mut() {
                    if *g == old_g {
                        *g = new_g;
                    }
                }
            }
        }
    }

    let mut result = Vec::new();
    for g in 0..n {
        let mut current_open = Vec::new();
        let mut current_close = Vec::new();
        for i in 0..n {
            if group[i] == g {
                current_open.push(brackets[i].0.clone());
                current_close.push(brackets[i].1.clone());
            }
        }
        if !current_open.is_empty() {
            result.push(InternalBracket {
                open: current_open,
                close: current_close,
            });
        }
    }
    result
}

/// `RichEditBrackets`: todos os grupos de colchetes de uma linguagem.
#[derive(Debug)]
pub struct RichEditBrackets {
    /// Os grupos de colchetes.
    pub brackets: Vec<Rc<RichEditBracket>>,
    /// Regex para achar todos os pares num texto (ver `getRegexForBrackets`).
    pub forward_regex: Rc<JsRegExp>,
    /// O mesmo, para buscar de trás para frente (ver `getReversedRegexForBrackets`).
    pub reversed_regex: Rc<JsRegExp>,
    /// O comprimento (em unidades UTF-16) do maior colchete.
    pub max_bracket_length: usize,
    /// Do texto de um colchete para o grupo dele.
    pub text_is_bracket: HashMap<JsString, Rc<RichEditBracket>>,
    /// Do texto de um colchete para "é uma abertura".
    pub text_is_open_bracket: HashMap<JsString, bool>,
}

impl RichEditBrackets {
    pub fn new(language_id: &str, brackets: &[CharacterPair]) -> Self {
        let brackets = group_fuzzy_brackets(brackets);

        let rich_brackets: Vec<Rc<RichEditBracket>> = brackets
            .iter()
            .enumerate()
            .map(|(index, b)| {
                Rc::new(RichEditBracket::new(
                    language_id,
                    index,
                    b.open.clone(),
                    b.close.clone(),
                    get_regex_for_bracket_pair(&b.open, &b.close, &brackets, index),
                    get_reversed_regex_for_bracket_pair(&b.open, &b.close, &brackets, index),
                ))
            })
            .collect();

        let forward_regex = Rc::new(get_regex_for_brackets(&rich_brackets));
        let reversed_regex = Rc::new(get_reversed_regex_for_brackets(&rich_brackets));

        let mut text_is_bracket = HashMap::new();
        let mut text_is_open_bracket = HashMap::new();
        let mut max_bracket_length = 0;
        for bracket in &rich_brackets {
            for open in &bracket.open {
                text_is_bracket.insert(open.clone(), bracket.clone());
                text_is_open_bracket.insert(open.clone(), true);
                max_bracket_length = max_bracket_length.max(open.len());
            }
            for close in &bracket.close {
                text_is_bracket.insert(close.clone(), bracket.clone());
                text_is_open_bracket.insert(close.clone(), false);
                max_bracket_length = max_bracket_length.max(close.len());
            }
        }

        Self {
            brackets: rich_brackets,
            forward_regex,
            reversed_regex,
            max_bracket_length,
            text_is_bracket,
            text_is_open_bracket,
        }
    }
}

fn collect_superstrings(
    str: &JsString,
    brackets: &[InternalBracket],
    current_index: usize,
    dest: &mut Vec<JsString>,
) {
    for (i, bracket) in brackets.iter().enumerate() {
        if i == current_index {
            continue;
        }
        for open in &bracket.open {
            if index_of(open.units(), str.units(), 0).is_some() {
                dest.push(open.clone());
            }
        }
        for close in &bracket.close {
            if index_of(close.units(), str.units(), 0).is_some() {
                dest.push(close.clone());
            }
        }
    }
}

fn unique(arr: Vec<JsString>) -> Vec<JsString> {
    if arr.len() <= 1 {
        return arr;
    }
    let mut seen = HashSet::new();
    arr.into_iter()
        .filter(|element| seen.insert(element.clone()))
        .collect()
}

/// Os colchetes de um grupo mais os que contêm algum deles (as "superstrings"), do maior para o menor.
fn pieces_with_superstrings(
    open: &[JsString],
    close: &[JsString],
    brackets: &[InternalBracket],
    current_index: usize,
) -> Vec<JsString> {
    let mut pieces: Vec<JsString> = open.iter().chain(close).cloned().collect();
    // O comprimento é o inicial: o que `collect_superstrings` acrescenta não é revisitado.
    for i in 0..pieces.len() {
        let piece = pieces[i].clone();
        collect_superstrings(&piece, brackets, current_index, &mut pieces);
    }
    let mut pieces = unique(pieces);
    pieces.sort_by_key(JsString::len);
    pieces.reverse();
    pieces
}

/// `getRegexForBracketPair`: a regex para buscar para frente um grupo de pares, ciente dos outros
/// colchetes. Com `['begin', 'end']` e `['if', 'end if']`, o `end` não pode ser confundido com o
/// começo de `end if`, então para o primeiro par a regex também mira `end if`:
/// `/(\bend if\b)|(\bend\b)|(\bif\b)/`.
fn get_regex_for_bracket_pair(
    open: &[JsString],
    close: &[JsString],
    brackets: &[InternalBracket],
    current_index: usize,
) -> JsRegExp {
    create_bracket_or_reg_exp(
        &pieces_with_superstrings(open, close, brackets, current_index),
        None,
    )
}

/// `getReversedRegexForBracketPair`: o mesmo para buscar de trás para frente (o texto e a regex ao
/// contrário): `/(\bfi dne\b)|(\bdne\b)|(\bfi\b)/`.
fn get_reversed_regex_for_bracket_pair(
    open: &[JsString],
    close: &[JsString],
    brackets: &[InternalBracket],
    current_index: usize,
) -> JsRegExp {
    let pieces: Vec<JsString> = pieces_with_superstrings(open, close, brackets, current_index)
        .iter()
        .map(|piece| to_reversed_string(piece.units()))
        .collect();
    create_bracket_or_reg_exp(&pieces, None)
}

/// Todos os colchetes de todos os grupos, sem repetição.
fn unique_pieces(brackets: &[Rc<RichEditBracket>]) -> Vec<JsString> {
    let pieces = brackets
        .iter()
        .flat_map(|bracket| bracket.open.iter().chain(&bracket.close))
        .cloned()
        .collect();
    unique(pieces)
}

/// `getRegexForBrackets`: a regex que mira todos os pares, como `/(\{)|(\})|(\bbegin\b)|(\bend\b)|(\bfor\b)/`.
fn get_regex_for_brackets(brackets: &[Rc<RichEditBracket>]) -> JsRegExp {
    create_bracket_or_reg_exp(&unique_pieces(brackets), None)
}

/// `getReversedRegexForBrackets`: o mesmo para o texto ao contrário, como
/// `/(\{)|(\})|(\bnigeb\b)|(\bdne\b)|(\brof\b)/`.
fn get_reversed_regex_for_brackets(brackets: &[Rc<RichEditBracket>]) -> JsRegExp {
    let pieces: Vec<JsString> = unique_pieces(brackets)
        .iter()
        .map(|piece| to_reversed_string(piece.units()))
        .collect();
    create_bracket_or_reg_exp(&pieces, None)
}

fn prepare_bracket_for_reg_exp(str: &JsString) -> Vec<u16> {
    // Este par usa letras, como `begin` e `end`.
    let insert_word_boundaries =
        !str.is_empty() && str.units().iter().all(|&u| is_word_unit(u) || u == 0x20);
    let escaped = escape_reg_exp_characters(str.units());
    if insert_word_boundaries {
        let mut out: Vec<u16> = "\\b".encode_utf16().collect();
        out.extend_from_slice(escaped.units());
        out.extend("\\b".encode_utf16());
        out
    } else {
        escaped.units().to_vec()
    }
}

/// `createBracketOrRegExp`.
pub fn create_bracket_or_reg_exp(pieces: &[JsString], options: Option<&RegExpOptions>) -> JsRegExp {
    let mut source: Vec<u16> = vec![u16::from(b'(')];
    for (i, piece) in pieces.iter().enumerate() {
        if i > 0 {
            source.extend(")|(".encode_utf16());
        }
        source.extend(prepare_bracket_for_reg_exp(piece));
    }
    source.push(u16::from(b')'));
    create_reg_exp(&source, true, &options.copied().unwrap_or_default())
}

/// `text.match(regex)` sem a flag `g`: o índice e o comprimento do primeiro casamento. Com `g` o
/// `String.prototype.match` devolve só os textos, e o índice vira 0.
fn match_first(regex: &JsRegExp, text: &[u16]) -> Option<(usize, usize)> {
    let global = regex.flags().global;
    if global {
        regex.set_last_index(0);
    }
    let found = regex.exec(text);
    if global {
        regex.set_last_index(0);
    }
    found.map(|m| (if global { 0 } else { m.start() }, m.end() - m.start()))
}

/// `BracketsUtils`.
pub struct BracketsUtils;

impl BracketsUtils {
    fn find_prev_bracket_in_text(
        reversed_bracket_regex: &JsRegExp,
        line_number: i32,
        reversed_text: &[u16],
        offset: i32,
    ) -> Option<Range> {
        let (index, match_length) = match_first(reversed_bracket_regex, reversed_text)?;
        let match_offset = reversed_text.len() as i32 - index as i32;
        let match_length = match_length as i32;
        let absolute_match_offset = offset + match_offset;

        Some(Range::new(
            line_number,
            absolute_match_offset - match_length + 1,
            line_number,
            absolute_match_offset + 1,
        ))
    }

    pub fn find_prev_bracket_in_range(
        reversed_bracket_regex: &JsRegExp,
        line_number: i32,
        line_text: &[u16],
        start_offset: i32,
        end_offset: i32,
    ) -> Option<Range> {
        // O JS não busca de trás para frente: busca para frente no texto ao contrário, com a regex ao contrário.
        let reversed_line_text = to_reversed_string(line_text);
        let len = line_text.len() as i64;
        let reversed_substr = substring(
            reversed_line_text.units(),
            len - i64::from(end_offset),
            len - i64::from(start_offset),
        );
        Self::find_prev_bracket_in_text(
            reversed_bracket_regex,
            line_number,
            reversed_substr,
            start_offset,
        )
    }

    pub fn find_next_bracket_in_text(
        bracket_regex: &JsRegExp,
        line_number: i32,
        text: &[u16],
        offset: i32,
    ) -> Option<Range> {
        let (index, match_length) = match_first(bracket_regex, text)?;
        if match_length == 0 {
            return None;
        }
        let absolute_match_offset = offset + index as i32;
        Some(Range::new(
            line_number,
            absolute_match_offset + 1,
            line_number,
            absolute_match_offset + 1 + match_length as i32,
        ))
    }

    pub fn find_next_bracket_in_range(
        bracket_regex: &JsRegExp,
        line_number: i32,
        line_text: &[u16],
        start_offset: i32,
        end_offset: i32,
    ) -> Option<Range> {
        let substr = substring(line_text, i64::from(start_offset), i64::from(end_offset));
        Self::find_next_bracket_in_text(bracket_regex, line_number, substr, start_offset)
    }
}
