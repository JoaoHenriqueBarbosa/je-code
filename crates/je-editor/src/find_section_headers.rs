//! Porte de `src/vs/editor/common/services/findSectionHeaders.ts`: acha os cabeçalhos de seção do modelo
//! (as regiões marcadas pelas regras de dobra da linguagem e os `MARK:` dados por uma regex).
//!
//! Decisões de tradução:
//! - O modelo chega pelo trait mínimo [`ISectionHeaderFinderTarget`], como no original; números de linha
//!   e coluna são `i32`, como no [`Range`], e o texto é UTF-16 ([`JsString`]).
//! - `markSectionHeaderRegex` é um [`JsString`]; um padrão inválido faz o `new RegExp` lançar, aqui um
//!   pânico com o [`Error`] da mensagem do motor.
//! - `String.prototype.match` sobre a regex de início de região vira `match_first`: com a flag `g` o
//!   original zera `lastIndex` e o deixa em 0 no fim.
//! - Um casamento vazio que não avança o `lastIndex` repete para sempre, como no original: a guarda
//!   `regExpLeadsToEndlessLoop` só cobre a regex que casa a cadeia vazia.

use je_base::JsString;
use je_base::errors::{self, Error};
use zyarr::Match;

use je_base::js_regexp::JsRegExp;
use crate::language_configuration::FoldingRules;
use crate::range::Range;
use crate::text_model_search::is_multiline_reg_exp_source;
use je_base::strings::reg_exp_leads_to_endless_loop;

const LINE_FEED: u16 = b'\n' as u16;
const DASH: u16 = b'-' as u16;

const CHUNK_SIZE: i32 = 100;
const MAX_SECTION_LINES: i32 = 5;

/// `ISectionHeaderFinderTarget`: o que a busca precisa do modelo.
pub trait ISectionHeaderFinderTarget {
    fn get_line_count(&self) -> i32;
    fn get_line_content(&self, line_number: i32) -> JsString;
}

/// `FindSectionHeaderOptions`.
#[derive(Clone, Debug)]
pub struct FindSectionHeaderOptions {
    pub folding_rules: Option<FoldingRules>,
    pub find_region_section_headers: bool,
    pub find_mark_section_headers: bool,
    pub mark_section_header_regex: JsString,
}

/// `SectionHeader`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionHeader {
    /// O local do texto do cabeçalho no modelo.
    pub range: Range,
    /// O texto do cabeçalho da seção.
    pub text: JsString,
    /// Se o cabeçalho inclui uma linha separadora.
    pub has_separator_line: bool,
    /// A seção deve ser omitida antes de desenhar se não estiver num comentário.
    pub should_be_in_comments: bool,
}

/// `findSectionHeaders(model, options)`.
pub fn find_section_headers(model: &dyn ISectionHeaderFinderTarget, options: &FindSectionHeaderOptions) -> Vec<SectionHeader> {
    let mut headers: Vec<SectionHeader> = Vec::new();
    let markers = options.folding_rules.as_ref().and_then(|rules| rules.markers.as_ref());
    if let (true, Some(markers)) = (options.find_region_section_headers, markers) {
        headers.extend(collect_region_headers(model, &markers.start));
    }
    if options.find_mark_section_headers {
        headers.extend(collect_mark_headers(model, options));
    }
    headers
}

/// `lineContent.match(regex)`, só o primeiro casamento (o `match[0]` e o `index` que o original usa).
fn match_first(regex: &JsRegExp, text: &[u16]) -> Option<Match> {
    if regex.flags().global {
        regex.set_last_index(0);
        let found = regex.exec(text);
        regex.set_last_index(0);
        found
    } else {
        regex.exec(text)
    }
}

fn collect_region_headers(model: &dyn ISectionHeaderFinderTarget, start: &JsRegExp) -> Vec<SectionHeader> {
    let mut region_headers: Vec<SectionHeader> = Vec::new();
    let end_line_number = model.get_line_count();
    for line_number in 1..=end_line_number {
        let line_content = model.get_line_content(line_number);
        let units = line_content.units();
        let Some(found) = match_first(start, units) else {
            continue;
        };
        let matched_length = found.end() - found.start();
        let range = Range {
            start_line_number: line_number,
            start_column: matched_length as i32 + 1,
            end_line_number: line_number,
            end_column: units.len() as i32 + 1,
        };
        if range.end_column > range.start_column {
            let (text, has_separator_line) = get_header_text(&units[matched_length..]);
            let section_header = SectionHeader {
                range,
                text,
                has_separator_line,
                should_be_in_comments: false,
            };
            if !section_header.text.is_empty() || section_header.has_separator_line {
                region_headers.push(section_header);
            }
        }
    }
    region_headers
}

/// `collectMarkHeaders(model, options)`.
pub fn collect_mark_headers(model: &dyn ISectionHeaderFinderTarget, options: &FindSectionHeaderOptions) -> Vec<SectionHeader> {
    let mut mark_headers: Vec<SectionHeader> = Vec::new();
    let end_line_number = model.get_line_count();

    // Valida a regex para evitar laços sem fim.
    let source = options.mark_section_header_regex.units();
    if trim_units(source).is_empty() {
        return mark_headers;
    }

    // Flags: `d` para os índices, `m` para `^` e `$` casarem em início e fim de linha, `s` (só quando o
    // padrão pode casar quebra de linha) para o `.` casar quebras.
    let multiline = is_multiline_reg_exp_source(source);
    let flags = if multiline { "gdms" } else { "gdm" };
    let regex = match JsRegExp::new(source, flags) {
        Ok(regex) => regex,
        Err(error) => errors::throw(Error::new(error.message())),
    };

    // Confere se a regex levaria a um laço sem fim.
    if reg_exp_leads_to_endless_loop(&regex) {
        return mark_headers;
    }

    // Processa o texto em pedaços que se sobrepõem, por desempenho.
    let mut start_line = 1;
    while start_line <= end_line_number {
        let end_line = (start_line + CHUNK_SIZE - 1).min(end_line_number);

        // Junta as linhas do pedaço atual.
        let mut text: Vec<u16> = Vec::new();
        for line in start_line..=end_line {
            if line > start_line {
                text.push(LINE_FEED);
            }
            text.extend_from_slice(model.get_line_content(line).units());
        }

        regex.set_last_index(0);

        while let Some(found) = regex.exec(&text) {
            let match_start = found.start();
            let match_end = found.end();

            // A linha onde o casamento começa: conta as quebras antes dele.
            let preceding_text = &text[..match_start];
            let line_offset = preceding_text.iter().filter(|&&unit| unit == LINE_FEED).count() as i32;
            let line_number = start_line + line_offset;

            // A altura do casamento, para conferir a sobreposição.
            let matched = &text[match_start..match_end];
            let match_height = matched.iter().filter(|&&unit| unit == LINE_FEED).count() as i32 + 1;
            let match_end_line = line_number + match_height - 1;

            // Coluna inicial: parte do início da linha que contém o casamento.
            let line_start_index = preceding_text
                .iter()
                .rposition(|&unit| unit == LINE_FEED)
                .map_or(0, |index| index + 1);
            let start_column = (match_start - line_start_index) as i32 + 1;

            // Coluna final: trata o casamento de várias linhas.
            let last_match_line_length = matched
                .iter()
                .rposition(|&unit| unit == LINE_FEED)
                .map_or(matched.len(), |index| matched.len() - index - 1);
            let end_column = if match_height == 1 {
                start_column + matched.len() as i32
            } else {
                last_match_line_length as i32 + 1
            };

            let range = Range {
                start_line_number: line_number,
                start_column,
                end_line_number: match_end_line,
                end_column,
            };

            let text2 = found
                .group("label")
                .map_or_else(JsString::new, |span| JsString::from(&text[span]));
            let has_separator_line = found.group("separator").is_some_and(|span| !span.is_empty());

            let section_header = SectionHeader {
                range,
                text: text2,
                has_separator_line,
                should_be_in_comments: true,
            };

            if !section_header.text.is_empty() || section_header.has_separator_line {
                // Só acrescenta se o anterior não termina na mesma linha.
                if mark_headers
                    .last()
                    .map_or(true, |previous| previous.range.end_line_number < section_header.range.start_line_number)
                {
                    mark_headers.push(section_header);
                }
            }

            // Move o `lastIndex` para depois do casamento atual.
            regex.set_last_index(match_end);
        }

        start_line += CHUNK_SIZE - MAX_SECTION_LINES;
    }

    mark_headers
}

/// `getHeaderText(text)`: o texto aparado, sem os traços das pontas, e se começava com traço.
fn get_header_text(text: &[u16]) -> (JsString, bool) {
    let text = trim_units(text);
    let has_separator_line = text.first() == Some(&DASH);
    // `text.replace(/^-+|-+$/g, '')`
    let without_leading = &text[text.iter().take_while(|&&unit| unit == DASH).count()..];
    let trailing = without_leading.iter().rev().take_while(|&&unit| unit == DASH).count();
    let trimmed = &without_leading[..without_leading.len() - trailing];
    (JsString::from(trimmed), has_separator_line)
}

/// `WhiteSpace` ou `LineTerminator` do ECMAScript (o que `String.prototype.trim` remove).
fn is_js_white_space(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000D
            | 0x0020
            | 0x00A0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

/// `String.prototype.trim` sobre unidades UTF-16.
fn trim_units(units: &[u16]) -> &[u16] {
    let start = units.iter().take_while(|&&unit| is_js_white_space(unit)).count();
    let units = &units[start..];
    let end = units.len() - units.iter().rev().take_while(|&&unit| is_js_white_space(unit)).count();
    &units[..end]
}
