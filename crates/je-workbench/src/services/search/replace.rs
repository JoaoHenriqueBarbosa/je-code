//! Porte de `src/vs/workbench/services/search/common/replace.ts`: `ReplacePattern`, a string de
//! substituição da busca com os `$n`, os `\n`/`\t` e as operações de caixa (`\u`, `\U`, `\l`, `\L`).
//!
//! Notas de tradução:
//! - O construtor sobrecarregado do original vira dois: [`ReplacePattern::from_pattern_info`] (com um
//!   `IPatternInfo`) e [`ReplacePattern::from_reg_exp`] (com `parseParameters` e um `RegExp`).
//! - O `RegExp` do JavaScript é o [`JsRegExp`] do je-base, criado por `create_reg_exp` do `strings.ts`. O
//!   `SyntaxError` do `new RegExp(...)` sai como `Err(RegExpError)`; a cadeia de busca vazia lança (pânico)
//!   dentro do `create_reg_exp`, como as demais exceções do porte.
//! - `String.prototype.replace` com uma `RegExp` não global e uma string de substituição (a expansão de
//!   `$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn` e `$<nome>`) não existe no je-base (`js_regexp::replace_all` é
//!   só o global com função), então está em [`replace_first`].
//! - Em `replaceWithCaseOperations`, um `\u` ou `\l` além do fim do grupo (`replacement[i]` indefinido)
//!   lançaria `TypeError` no original; aqui não acrescenta nada.

use je_base::char_code as cc;
use je_base::js_regexp::{JsRegExp, match_all};
use je_base::js_string::{JsString, substring};
use je_base::search::build_replace_string_with_case_preserved;
use je_base::strings::{RegExpOptions, create_reg_exp};
use zyarr::{Match, RegExp, RegExpError};

use super::search::PatternInfo;

/// A `/([\s\S]*?)((?:\\[uUlL])+?|)(\$[0-9]+)([\s\S]*?)/g` do original.
const CASE_OPS_PATTERN: &str = r"([\s\S]*?)((?:\\[uUlL])+?|)(\$[0-9]+)([\s\S]*?)";

fn is_digit(unit: u16) -> bool {
    (cc::Digit0..=cc::Digit9).contains(&unit)
}

/// `String.prototype.substr(start, length)` com `start >= 0`.
fn substr(units: &[u16], start: usize, length: i64) -> &[u16] {
    let start = start.min(units.len());
    if length <= 0 {
        return &[];
    }
    let end = start
        .saturating_add(usize::try_from(length).unwrap_or(usize::MAX))
        .min(units.len());
    &units[start..end]
}

fn push_str(out: &mut JsString, text: &str) {
    out.push_units(&text.encode_utf16().collect::<Vec<u16>>());
}

/// `GetSubstitution`: expande `$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn` e `$<nome>` na `replacement` de um
/// casamento de `input`.
fn get_substitution(found: &Match, input: &[u16], replacement: &[u16]) -> JsString {
    let position = found.start();
    let match_end = found.end();
    let capture_count = found.captures.len() - 1;
    let len = replacement.len();
    let mut out = JsString::new();
    let mut i = 0;
    while i < len {
        let unit = replacement[i];
        if unit != cc::DollarSign || i + 1 >= len {
            out.push_unit(unit);
            i += 1;
            continue;
        }
        let next = replacement[i + 1];
        match next {
            cc::DollarSign => {
                out.push_unit(cc::DollarSign);
                i += 2;
            }
            cc::Ampersand => {
                out.push_units(&input[position..match_end]);
                i += 2;
            }
            cc::BackTick => {
                out.push_units(&input[..position]);
                i += 2;
            }
            cc::SingleQuote => {
                out.push_units(&input[match_end.min(input.len())..]);
                i += 2;
            }
            _ if is_digit(next) => {
                let first_digit = usize::from(next - cc::Digit0);
                let mut digit_count = 1;
                let mut index = first_digit;
                if let Some(&second) = replacement.get(i + 2).filter(|&&u| is_digit(u)) {
                    digit_count = 2;
                    index = first_digit * 10 + usize::from(second - cc::Digit0);
                    if index > capture_count {
                        digit_count = 1;
                        index = first_digit;
                    }
                }
                if (1..=capture_count).contains(&index) {
                    if let Some(range) = found.captures.get(index).and_then(Option::as_ref) {
                        out.push_units(&input[range.clone()]);
                    }
                } else {
                    out.push_units(&replacement[i..i + 1 + digit_count]);
                }
                i += 1 + digit_count;
            }
            cc::LessThan if !found.groups.is_empty() => {
                match replacement[i + 2..]
                    .iter()
                    .position(|&u| u == cc::GreaterThan)
                {
                    Some(offset) => {
                        let end = i + 2 + offset;
                        let name = String::from_utf16_lossy(&replacement[i + 2..end]);
                        if let Some(range) = found.group(&name) {
                            out.push_units(&input[range]);
                        }
                        i = end + 1;
                    }
                    None => {
                        out.push_unit(cc::DollarSign);
                        i += 1;
                    }
                }
            }
            _ => {
                out.push_unit(cc::DollarSign);
                i += 1;
            }
        }
    }
    out
}

/// `text.replace(regExp, replacement)` com uma `RegExp` não global: troca só o primeiro casamento.
fn replace_first(text: &[u16], reg_exp: &JsRegExp, replacement: &[u16]) -> JsString {
    let Some(found) = reg_exp.exec(text) else {
        return JsString::from(text);
    };
    let mut out = JsString::from(&text[..found.start()]);
    out.push_units(get_substitution(&found, text, replacement).units());
    out.push_units(&text[found.end()..]);
    out
}

/// `/\\[uUlL]/.test(text)`.
fn contains_case_operation(text: &[u16]) -> bool {
    text.windows(2)
        .any(|pair| pair[0] == cc::Backslash && matches!(pair[1], cc::u | cc::U | cc::l | cc::L))
}

fn convert_case(units: &[u16], upper: bool) -> JsString {
    let text = JsString::from(units);
    if upper {
        text.to_upper_case()
    } else {
        text.to_lower_case()
    }
}

/// `parseInt(digits)` como índice de vetor: o que passa de `usize` cai fora de qualquer vetor.
fn parse_index(digits: &[u16]) -> usize {
    digits.iter().fold(0usize, |acc, &unit| {
        acc.saturating_mul(10)
            .saturating_add(usize::from(unit - cc::Digit0))
    })
}

pub struct ReplacePattern {
    replace_pattern: JsString,
    has_parameters: bool,
    reg_exp: JsRegExp,
    case_ops_reg_exp: RegExp,
}

impl ReplacePattern {
    /// `new ReplacePattern(replaceString, searchPatternInfo)`.
    pub fn from_pattern_info(
        replace_string: impl Into<JsString>,
        search_pattern_info: &PatternInfo,
    ) -> Result<Self, RegExpError> {
        let is_reg_exp = search_pattern_info.is_reg_exp.unwrap_or(false);
        let reg_exp = create_reg_exp(
            search_pattern_info.pattern.units(),
            is_reg_exp,
            &RegExpOptions {
                match_case: search_pattern_info.is_case_sensitive.unwrap_or(false),
                whole_word: search_pattern_info.is_word_match.unwrap_or(false),
                multiline: search_pattern_info.is_multiline.unwrap_or(false),
                global: false,
                unicode: true,
            },
        )?;
        Self::build(replace_string.into(), is_reg_exp, reg_exp)
    }

    /// `new ReplacePattern(replaceString, parseParameters, regEx)`.
    pub fn from_reg_exp(
        replace_string: impl Into<JsString>,
        parse_parameters: bool,
        reg_exp: JsRegExp,
    ) -> Result<Self, RegExpError> {
        Self::build(replace_string.into(), parse_parameters, reg_exp)
    }

    fn build(
        replace_string: JsString,
        parse_parameters: bool,
        mut reg_exp: JsRegExp,
    ) -> Result<Self, RegExpError> {
        let mut replace_pattern = replace_string;
        let mut has_parameters = false;
        if parse_parameters {
            (replace_pattern, has_parameters) = parse_replace_string(&replace_pattern);
        }

        let flags = reg_exp.flags();
        if flags.global {
            reg_exp = create_reg_exp(
                reg_exp.source(),
                true,
                &RegExpOptions {
                    match_case: !flags.ignore_case,
                    whole_word: false,
                    multiline: flags.multiline,
                    global: false,
                    unicode: false,
                },
            )?;
        }
        Ok(Self {
            replace_pattern,
            has_parameters,
            reg_exp,
            case_ops_reg_exp: RegExp::from_str(CASE_OPS_PATTERN, "g")?,
        })
    }

    pub fn has_parameters(&self) -> bool {
        self.has_parameters
    }

    pub fn pattern(&self) -> &JsString {
        &self.replace_pattern
    }

    pub fn reg_exp(&self) -> &JsRegExp {
        &self.reg_exp
    }

    /// Returns the replace string for the first match in the given text.
    /// If text has no matches then returns `None`.
    pub fn get_replace_string(&self, text: &[u16], preserve_case: bool) -> Option<JsString> {
        let found = self.reg_exp.exec(text)?;
        let captures: Vec<JsString> = found
            .captures
            .iter()
            .map(|capture| {
                capture
                    .as_ref()
                    .map_or_else(JsString::new, |range| JsString::from(&text[range.clone()]))
            })
            .collect();
        if self.has_parameters {
            let replace_string = self.replace_with_case_operations(
                text,
                &self.build_replace_string(Some(captures.as_slice()), preserve_case),
            );
            if captures[0].units() == text {
                return Some(replace_string);
            }
            let length =
                captures[0].len() as i64 - (text.len() as i64 - replace_string.len() as i64);
            return Some(JsString::from(substr(
                replace_string.units(),
                found.start(),
                length,
            )));
        }
        Some(self.build_replace_string(Some(captures.as_slice()), preserve_case))
    }

    /// `replaceWithCaseOperations` applies case operations to relevant replacement strings and applies
    /// the affected $N arguments. It then passes unaffected $N arguments through to string.replace().
    ///
    /// `\u` upper-cases one character in a match, `\U` upper-cases ALL remaining characters in a match,
    /// `\l` lower-cases one character in a match, `\L` lower-cases ALL remaining characters in a match.
    fn replace_with_case_operations(&self, text: &[u16], replace_string: &JsString) -> JsString {
        // Short-circuit the common path.
        if !contains_case_operation(replace_string.units()) {
            return replace_first(text, &self.reg_exp, replace_string.units());
        }
        // Store the values of the search parameters.
        let Some(first_match) = self.reg_exp.exec(text) else {
            return replace_first(text, &self.reg_exp, replace_string.units());
        };

        let units = replace_string.units();
        let mut new_replace_string = JsString::new();
        let mut last_index = 0;
        let mut last_match_len = 0;
        // For each annotated $N, perform text processing on the parameters and perform the substitution.
        for pat_match in match_all(&self.case_ops_reg_exp, units) {
            let group = |n: usize| {
                pat_match.captures[n]
                    .as_ref()
                    .map_or(&units[..0], |range| &units[range.clone()])
            };
            last_index = pat_match.start();
            let full_match = group(0);
            last_match_len = full_match.len();
            let case_ops = group(2); // \u, \l\u, etc.
            let money = group(3); // $1, $2, etc.

            if case_ops.is_empty() {
                new_replace_string.push_units(full_match);
                continue;
            }
            let replacement: &[u16] = first_match
                .captures
                .get(parse_index(&money[1..]))
                .and_then(Option::as_ref)
                .map_or(&text[..0], |range| &text[range.clone()]);
            if replacement.is_empty() {
                new_replace_string.push_units(full_match);
                continue;
            }
            let replacement_len = replacement.len();

            new_replace_string.push_units(group(1)); // prefix
            let ops: Vec<u16> = case_ops
                .iter()
                .copied()
                .filter(|&unit| unit != cc::Backslash)
                .collect();
            let mut i = 0;
            while i < ops.len() {
                let rest = replacement.get(i..).unwrap_or_default();
                match ops[i] {
                    cc::U => {
                        new_replace_string.push_units(convert_case(rest, true).units());
                        i = replacement_len;
                    }
                    cc::u => {
                        if let Some(unit) = replacement.get(i) {
                            new_replace_string.push_units(convert_case(&[*unit], true).units());
                        }
                    }
                    cc::L => {
                        new_replace_string.push_units(convert_case(rest, false).units());
                        i = replacement_len;
                    }
                    cc::l => {
                        if let Some(unit) = replacement.get(i) {
                            new_replace_string.push_units(convert_case(&[*unit], false).units());
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            // Append any remaining replacement string content not covered by case operations.
            if i < replacement_len {
                new_replace_string.push_units(&replacement[i..]);
            }

            new_replace_string.push_units(group(4)); // suffix
        }

        // Append any remaining trailing content after the final regex match.
        new_replace_string.push_units(units.get(last_index + last_match_len..).unwrap_or_default());

        replace_first(text, &self.reg_exp, new_replace_string.units())
    }

    /// `matches` é o resultado de um `exec` (o texto casado em `matches[0]`).
    pub fn build_replace_string(
        &self,
        matches: Option<&[JsString]>,
        preserve_case: bool,
    ) -> JsString {
        if preserve_case {
            build_replace_string_with_case_preserved(matches, &self.replace_pattern)
        } else {
            self.replace_pattern.clone()
        }
    }
}

/// \n => LF
/// \t => TAB
/// \\ => \
/// $0 => $& (see https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/String/replace#Specifying_a_string_as_a_parameter)
/// everything else stays untouched
///
/// Devolve o texto reescrito e o `_hasParameters` que o original grava no objeto.
fn parse_replace_string(original: &JsString) -> (JsString, bool) {
    let mut has_parameters = false;
    if original.is_empty() {
        return (original.clone(), has_parameters);
    }

    let replace_string = original.units();
    let len = replace_string.len();
    let mut substr_from = 0usize;
    let mut result = JsString::new();
    // `replaceString.substring(substrFrom, i - 1)` seguido do caractere novo.
    let mut splice = |result: &mut JsString, i: usize, replacement: &str| {
        result.push_units(substring(replace_string, substr_from as i64, i as i64 - 1));
        push_str(result, replacement);
        substr_from = i + 1;
    };

    let mut i = 0;
    while i < len {
        let ch_code = replace_string[i];

        if ch_code == cc::Backslash {
            // move to next char
            i += 1;

            if i >= len {
                // string ends with a \
                break;
            }

            let replace_with_character = match replace_string[i] {
                // \\ => \
                cc::Backslash => Some("\\"),
                // \n => LF
                cc::n => Some("\n"),
                // \t => TAB
                cc::t => Some("\t"),
                _ => None,
            };

            if let Some(character) = replace_with_character {
                splice(&mut result, i, character);
            }
        }

        if ch_code == cc::DollarSign {
            // move to next char
            i += 1;

            if i >= len {
                // string ends with a $
                break;
            }

            let next_ch_code = replace_string[i];
            let mut replace_with_character = None;

            match next_ch_code {
                cc::Digit0 => {
                    // $0 => $&
                    replace_with_character = Some("$&");
                    has_parameters = true;
                }
                cc::BackTick | cc::SingleQuote => {
                    has_parameters = true;
                }
                _ => {
                    // check if it is a valid string parameter $n (0 <= n <= 99). $0 is already handled by now.
                    if between(next_ch_code, cc::Digit1, cc::Digit9) {
                        if i == len - 1 {
                            has_parameters = true;
                        } else {
                            i += 1;
                            if !between(replace_string[i], cc::Digit0, cc::Digit9) {
                                has_parameters = true;
                                i -= 1;
                            } else if i == len - 1 {
                                has_parameters = true;
                            } else {
                                i += 1;
                                if !between(replace_string[i], cc::Digit0, cc::Digit9) {
                                    has_parameters = true;
                                    i -= 1;
                                }
                            }
                        }
                    }
                }
            }

            if let Some(character) = replace_with_character {
                splice(&mut result, i, character);
            }
        }
        i += 1;
    }

    if substr_from == 0 {
        // no replacement occurred
        return (original.clone(), has_parameters);
    }

    result.push_units(substring(replace_string, substr_from as i64, len as i64));
    (result, has_parameters)
}

fn between(value: u16, from: u16, to: u16) -> bool {
    from <= value && value <= to
}
