//! Porte de `src/vs/editor/contrib/find/browser/replacePattern.ts`: a leitura da cadeia de substituição
//! (`\n`, `\t`, `\\`, `\u`, `\U`, `\l`, `\L`, `$$`, `$&`, `$n`) e a montagem do texto substituído a partir
//! das capturas de um `RegExp.exec`.
//!
//! As capturas são `Option<JsString>` por posição, como o `string[]` do `RegExpExecArray` (uma captura que
//! não participou do casamento é `undefined`, aqui `None`). Os índices contam unidades UTF-16.
//! Divergência: o `matches` de `buildReplaceString` sem casamento (`null`) é `None`, e o de
//! `buildReplaceStringWithCasePreserved` (que não distingue `undefined`) recebe a captura ausente como
//! cadeia vazia.

use je_base::JsString;
use je_base::js_string::substring;
use je_base::search::build_replace_string_with_case_preserved;

const BACKSLASH: u16 = b'\\' as u16;
const DOLLAR_SIGN: u16 = b'$' as u16;
const AMPERSAND: u16 = b'&' as u16;
const DIGIT_0: u16 = b'0' as u16;
const DIGIT_1: u16 = b'1' as u16;
const DIGIT_9: u16 = b'9' as u16;

/// O estado de um `ReplacePattern`: inteiramente estático (`StaticValueReplacePattern`) ou com peças
/// dinâmicas (`DynamicPiecesReplacePattern`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReplacePatternState {
    StaticValue(JsString),
    DynamicPieces(Vec<ReplacePiece>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacePattern {
    state: ReplacePatternState,
}

impl ReplacePattern {
    pub fn from_static_value(value: JsString) -> ReplacePattern {
        ReplacePattern::new(Some(vec![ReplacePiece::static_value(value)]))
    }

    pub fn has_replacement_patterns(&self) -> bool {
        matches!(self.state, ReplacePatternState::DynamicPieces(_))
    }

    pub fn new(pieces: Option<Vec<ReplacePiece>>) -> ReplacePattern {
        let state = match pieces {
            None => ReplacePatternState::StaticValue(JsString::new()),
            Some(pieces) if pieces.is_empty() => ReplacePatternState::StaticValue(JsString::new()),
            Some(pieces) if pieces.len() == 1 && pieces[0].static_value.is_some() => {
                ReplacePatternState::StaticValue(pieces[0].static_value.clone().unwrap_or_default())
            }
            Some(pieces) => ReplacePatternState::DynamicPieces(pieces),
        };
        ReplacePattern { state }
    }

    /// `buildReplaceString(matches, preserveCase)`: `matches` é o resultado de um `RegExp.exec` (as
    /// capturas, com o texto casado em `matches[0]`), ou `None` quando nada casou.
    pub fn build_replace_string(&self, matches: Option<&[Option<JsString>]>, preserve_case: bool) -> JsString {
        let pieces = match &self.state {
            ReplacePatternState::StaticValue(static_value) => {
                if preserve_case {
                    let plain: Option<Vec<JsString>> =
                        matches.map(|m| m.iter().map(|capture| capture.clone().unwrap_or_default()).collect());
                    return build_replace_string_with_case_preserved(plain.as_deref(), static_value);
                }
                return static_value.clone();
            }
            ReplacePatternState::DynamicPieces(pieces) => pieces,
        };

        let mut result = JsString::new();
        for piece in pieces {
            if let Some(static_value) = &piece.static_value {
                // static value ReplacePiece
                result.push_units(static_value.units());
                continue;
            }

            // match index ReplacePiece
            let mut matched = ReplacePattern::substitute(piece.match_index, matches);
            if let Some(case_ops) = piece.case_ops.as_ref().filter(|ops| !ops.is_empty()) {
                matched = replace_with_case_ops(&matched, case_ops);
            }
            result.push_units(matched.units());
        }

        result
    }

    fn substitute(match_index: i32, matches: Option<&[Option<JsString>]>) -> JsString {
        let Some(matches) = matches else {
            return JsString::new();
        };
        if match_index == 0 {
            return matches[0].clone().unwrap_or_default();
        }

        let mut match_index = match_index;
        let mut remainder: Vec<u16> = Vec::new();
        while match_index > 0 {
            if (match_index as usize) < matches.len() {
                // A match can be undefined
                let mut matched = matches[match_index as usize].clone().unwrap_or_default();
                matched.push_units(&remainder);
                return matched;
            }
            remainder.insert(0, DIGIT_0 + (match_index % 10) as u16);
            match_index /= 10;
        }
        let mut result = JsString::from("$");
        result.push_units(&remainder);
        result
    }
}

/// Aplica os operadores de caixa (`u`, `U`, `l`, `L`) ao texto casado, unidade por unidade: `u` e `l`
/// valem para uma unidade e passam ao operador seguinte; `U` e `L` nunca avançam, então valem para todas
/// as unidades restantes.
fn replace_with_case_ops(matched: &JsString, case_ops: &[char]) -> JsString {
    let units = matched.units();
    let mut repl = JsString::new();
    let mut op_idx = 0;
    for idx in 0..units.len() {
        if op_idx >= case_ops.len() {
            repl.push_units(&units[idx..]);
            break;
        }
        let single = JsString::from_units(vec![units[idx]]);
        match case_ops[op_idx] {
            'U' => repl.push_units(single.to_upper_case().units()),
            'u' => {
                repl.push_units(single.to_upper_case().units());
                op_idx += 1;
            }
            'L' => repl.push_units(single.to_lower_case().units()),
            'l' => {
                repl.push_units(single.to_lower_case().units());
                op_idx += 1;
            }
            _ => repl.push_unit(units[idx]),
        }
    }
    repl
}

/// A replace piece can either be a static string or an index to a specific match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacePiece {
    pub static_value: Option<JsString>,
    /// `-1` nas peças estáticas.
    pub match_index: i32,
    pub case_ops: Option<Vec<char>>,
}

impl ReplacePiece {
    pub fn static_value(value: JsString) -> ReplacePiece {
        ReplacePiece::new(Some(value), -1, &[])
    }

    pub fn match_index(index: i32) -> ReplacePiece {
        ReplacePiece::new(None, index, &[])
    }

    pub fn case_ops(index: i32, case_ops: &[char]) -> ReplacePiece {
        ReplacePiece::new(None, index, case_ops)
    }

    fn new(static_value: Option<JsString>, match_index: i32, case_ops: &[char]) -> ReplacePiece {
        ReplacePiece {
            static_value,
            match_index,
            case_ops: if case_ops.is_empty() { None } else { Some(case_ops.to_vec()) },
        }
    }
}

struct ReplacePieceBuilder<'a> {
    source: &'a [u16],
    last_char_index: i64,
    result: Vec<ReplacePiece>,
    current_static_piece: Vec<u16>,
}

impl<'a> ReplacePieceBuilder<'a> {
    fn new(source: &'a [u16]) -> Self {
        Self {
            source,
            last_char_index: 0,
            result: Vec::new(),
            current_static_piece: Vec::new(),
        }
    }

    fn emit_unchanged(&mut self, to_char_index: i64) {
        let unchanged = substring(self.source, self.last_char_index, to_char_index);
        self.current_static_piece.extend_from_slice(unchanged);
        self.last_char_index = to_char_index;
    }

    fn emit_static(&mut self, value: &[u16], to_char_index: i64) {
        self.current_static_piece.extend_from_slice(value);
        self.last_char_index = to_char_index;
    }

    fn flush_static_piece(&mut self) {
        if !self.current_static_piece.is_empty() {
            let piece = std::mem::take(&mut self.current_static_piece);
            self.result.push(ReplacePiece::static_value(JsString::from_units(piece)));
        }
    }

    fn emit_match_index(&mut self, index: i32, to_char_index: i64, case_ops: &[char]) {
        self.flush_static_piece();
        self.result.push(ReplacePiece::case_ops(index, case_ops));
        self.last_char_index = to_char_index;
    }

    fn finalize(mut self) -> ReplacePattern {
        self.emit_unchanged(self.source.len() as i64);
        self.flush_static_piece();
        ReplacePattern::new(Some(self.result))
    }
}

/// ```txt
/// \n          => inserts a LF
/// \t          => inserts a TAB
/// \\          => inserts a "\".
/// \u          => upper-cases one character in a match.
/// \U          => upper-cases ALL remaining characters in a match.
/// \l          => lower-cases one character in a match.
/// \L          => lower-cases ALL remaining characters in a match.
/// $$          => inserts a "$".
/// $& and $0   => inserts the matched substring.
/// $n          => Where n is a non-negative integer lesser than 100, inserts the nth parenthesized submatch string
/// everything else stays untouched
/// ```
///
/// Veja também `String.prototype.replace` no MDN (especificar uma cadeia como parâmetro).
pub fn parse_replace_string(replace_string: &[u16]) -> ReplacePattern {
    if replace_string.is_empty() {
        return ReplacePattern::new(None);
    }

    let mut case_ops: Vec<char> = Vec::new();
    let mut result = ReplacePieceBuilder::new(replace_string);

    let len = replace_string.len();
    let mut i = 0;
    while i < len {
        let ch_code = replace_string[i];

        if ch_code == BACKSLASH {
            // move to next char
            i += 1;

            if i >= len {
                // string ends with a \
                break;
            }

            let next_ch_code = replace_string[i];
            let at = i as i64;

            match next_ch_code {
                // \\ => inserts a "\"
                BACKSLASH => {
                    result.emit_unchanged(at - 1);
                    result.emit_static(&[BACKSLASH], at + 1);
                }
                // \n => inserts a LF
                c if c == u16::from(b'n') => {
                    result.emit_unchanged(at - 1);
                    result.emit_static(&[u16::from(b'\n')], at + 1);
                }
                // \t => inserts a TAB
                c if c == u16::from(b't') => {
                    result.emit_unchanged(at - 1);
                    result.emit_static(&[u16::from(b'\t')], at + 1);
                }
                // Case modification of string replacements, patterned after Boost, but only applied
                // to the replacement text, not subsequent content.
                // \u => upper-cases one character. \U => upper-cases ALL following characters.
                // \l => lower-cases one character. \L => lower-cases ALL following characters.
                c if c == u16::from(b'u') || c == u16::from(b'U') || c == u16::from(b'l') || c == u16::from(b'L') => {
                    result.emit_unchanged(at - 1);
                    result.emit_static(&[], at + 1);
                    case_ops.push(char::from(c as u8));
                }
                _ => {}
            }

            i += 1;
            continue;
        }

        if ch_code == DOLLAR_SIGN {
            // move to next char
            i += 1;

            if i >= len {
                // string ends with a $
                break;
            }

            let next_ch_code = replace_string[i];
            let at = i as i64;

            if next_ch_code == DOLLAR_SIGN {
                // $$ => inserts a "$"
                result.emit_unchanged(at - 1);
                result.emit_static(&[DOLLAR_SIGN], at + 1);
                i += 1;
                continue;
            }

            if next_ch_code == DIGIT_0 || next_ch_code == AMPERSAND {
                // $& and $0 => inserts the matched substring.
                result.emit_unchanged(at - 1);
                result.emit_match_index(0, at + 1, &case_ops);
                case_ops.clear();
                i += 1;
                continue;
            }

            if (DIGIT_1..=DIGIT_9).contains(&next_ch_code) {
                // $n
                let mut match_index = i32::from(next_ch_code - DIGIT_0);

                // peek next char to probe for $nn
                if i + 1 < len {
                    let next_next_ch_code = replace_string[i + 1];
                    if (DIGIT_0..=DIGIT_9).contains(&next_next_ch_code) {
                        // $nn

                        // move to next char
                        i += 1;
                        match_index = match_index * 10 + i32::from(next_next_ch_code - DIGIT_0);

                        let at = i as i64;
                        result.emit_unchanged(at - 2);
                        result.emit_match_index(match_index, at + 1, &case_ops);
                        case_ops.clear();
                        i += 1;
                        continue;
                    }
                }

                result.emit_unchanged(at - 1);
                result.emit_match_index(match_index, at + 1, &case_ops);
                case_ops.clear();
                i += 1;
                continue;
            }
        }

        i += 1;
    }

    result.finalize()
}
