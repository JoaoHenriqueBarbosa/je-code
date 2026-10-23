//! Porte de `src/vs/editor/common/model/textModelSearch.ts`: `SearchParams` (a montagem da regex de
//! busca), o `Searcher` (varredura com `lastIndex`), `isValidMatch` (fronteira de palavra),
//! `createFindMatch` e `TextModelSearch` (busca de todos os casamentos, do próximo e do anterior sobre um
//! `TextModel`).
//!
//! O `createRegExp` e o `regExpLeadsToEndlessLoop` do `strings.ts` estão em `je_base::strings`.

use crate::cursor_common::CursorSimpleModel;
use je_base::JsString;
use je_base::char_code;
use je_base::strings::{RegExpOptions, create_reg_exp};
use je_base::js_string;
use je_base::strings;
use zyarr::Match;

use je_base::js_regexp::JsRegExp;
use crate::model::{EndOfLinePreference, Eol, FindMatch, SearchData};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;
use crate::word_character_classifier::{WordCharacterClass, WordCharacterClassifier, get_map_for_word_separators};

const LIMIT_FIND_COUNT: usize = 999;

// TODO(integração): mover para je_base::strings (`RegExpOptions`, `createRegExp` e
// `regExpLeadsToEndlessLoop`), junto do `JsRegExp`.

/// `SearchParams`: o que o usuário pediu para procurar.
#[derive(Clone, Debug)]
pub struct SearchParams {
    pub search_string: JsString,
    pub is_regex: bool,
    pub match_case: bool,
    pub word_separators: Option<JsString>,
}

impl SearchParams {
    pub fn new(search_string: JsString, is_regex: bool, match_case: bool, word_separators: Option<&str>) -> Self {
        Self {
            search_string,
            is_regex,
            match_case,
            word_separators: word_separators.map(JsString::from),
        }
    }

    /// `parseSearchRequest()`: `None` quando a busca é vazia ou a regex é inválida.
    pub fn parse_search_request(&self) -> Option<SearchData> {
        let search_string = self.search_string.units();
        if search_string.is_empty() {
            return None;
        }

        // Tenta montar uma regex a partir dos parâmetros.
        let multiline = if self.is_regex {
            is_multiline_reg_exp_source(search_string)
        } else {
            search_string.contains(&char_code::LineFeed)
        };

        let regex = create_reg_exp(
            search_string,
            self.is_regex,
            &RegExpOptions {
                match_case: self.match_case,
                whole_word: false,
                multiline,
                global: true,
                unicode: true,
            },
        )
        .ok()?;

        let mut can_use_simple_search = !self.is_regex && !multiline;
        if can_use_simple_search && self.search_string.to_lower_case() != self.search_string.to_upper_case() {
            // A caixa pode fazer diferença.
            can_use_simple_search = self.match_case;
        }

        let word_separators = self
            .word_separators
            .as_ref()
            .filter(|separators| !separators.is_empty())
            .map(|separators| get_map_for_word_separators(separators.units(), &[]));
        Some(SearchData::new(
            regex,
            word_separators,
            can_use_simple_search.then(|| self.search_string.clone()),
        ))
    }
}

/// `isMultilineRegexSource(searchString)`: o padrão pode casar uma quebra de linha (`\n` literal, ou os
/// escapes `\n`, `\r` e `\W`).
pub fn is_multiline_reg_exp_source(search_string: &[u16]) -> bool {
    let len = search_string.len();
    let mut i = 0;
    while i < len {
        let ch_code = search_string[i];

        if ch_code == char_code::LineFeed {
            return true;
        }

        if ch_code == char_code::Backslash {
            // Vai para o próximo caractere.
            i += 1;

            if i >= len {
                // A cadeia termina com `\`.
                break;
            }

            let next_ch_code = search_string[i];
            if next_ch_code == u16::from(b'n') || next_ch_code == u16::from(b'r') || next_ch_code == u16::from(b'W') {
                return true;
            }
        }
        i += 1;
    }

    false
}

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

/// `LineFeedCounter`: quantos `\n` há antes de um deslocamento, por busca binária.
struct LineFeedCounter {
    line_feeds_offsets: Vec<usize>,
}

impl LineFeedCounter {
    fn new(text: &[u16]) -> Self {
        let line_feeds_offsets = text
            .iter()
            .enumerate()
            .filter(|&(_, &unit)| unit == char_code::LineFeed)
            .map(|(i, _)| i)
            .collect();
        Self { line_feeds_offsets }
    }

    fn find_line_feed_count_before_offset(&self, offset: usize) -> usize {
        let line_feeds_offsets = &self.line_feeds_offsets;
        if line_feeds_offsets.is_empty() {
            // Nenhuma quebra de linha.
            return 0;
        }

        if offset <= line_feeds_offsets[0] {
            // Antes da primeira quebra de linha.
            return 0;
        }

        let mut min: isize = 0;
        let mut max: isize = line_feeds_offsets.len() as isize - 1;
        while min < max {
            let mid = min + (max - min) / 2;

            if line_feeds_offsets[mid as usize] >= offset {
                max = mid - 1;
            } else if line_feeds_offsets[mid as usize + 1] >= offset {
                // Achou.
                min = mid;
                max = mid;
            } else {
                min = mid + 1;
            }
        }
        min as usize + 1
    }
}

/// `TextModelSearch`: a busca sobre um `TextModel`.
pub struct TextModelSearch;

impl TextModelSearch {
    /// `findMatches(model, searchParams, searchRange, captureMatches, limitResultCount)`.
    pub fn find_matches(
        model: &TextModel,
        search_params: &SearchParams,
        search_range: &Range,
        capture_matches: bool,
        limit_result_count: usize,
    ) -> Vec<FindMatch> {
        let Some(search_data) = search_params.parse_search_request() else {
            return Vec::new();
        };

        if search_data.regex.flags().multiline {
            let mut searcher = Searcher::new(search_data.word_separators.as_deref(), &search_data.regex);
            return Self::do_find_matches_multiline(model, search_range, &mut searcher, capture_matches, limit_result_count);
        }
        Self::do_find_matches_line_by_line(model, search_range, &search_data, capture_matches, limit_result_count)
    }

    /// `_getMultilineMatchRange`: a busca de várias linhas sempre roda sobre as linhas unidas por `\n`;
    /// num modelo CRLF é preciso somar um `\r` para cada `\n` antes do casamento e dentro dele.
    fn get_multiline_match_range(
        model: &TextModel,
        delta_offset: usize,
        lf_counter: Option<&LineFeedCounter>,
        match_index: usize,
        match_length: usize,
    ) -> Range {
        let (start_offset, end_offset) = match lf_counter {
            Some(lf_counter) => {
                let line_feed_count_before_match = lf_counter.find_line_feed_count_before_offset(match_index);
                // Soma tantos `\r` quantos `\n` houve.
                let start_offset = delta_offset + match_index + line_feed_count_before_match;
                let line_feed_count_before_end_of_match = lf_counter.find_line_feed_count_before_offset(match_index + match_length);
                let line_feed_count_in_match = line_feed_count_before_end_of_match - line_feed_count_before_match;
                (start_offset, start_offset + match_length + line_feed_count_in_match)
            }
            None => {
                let start_offset = delta_offset + match_index;
                (start_offset, start_offset + match_length)
            }
        };

        let start_position = model.get_position_at(start_offset as i64);
        let end_position = model.get_position_at(end_offset as i64);
        Range::new(
            start_position.line_number,
            start_position.column,
            end_position.line_number,
            end_position.column,
        )
    }

    /// O texto de `range` unido por `\n` e, num modelo CRLF, o contador que compensa os `\r`.
    fn multiline_text(model: &TextModel, range: &Range) -> (JsString, Option<LineFeedCounter>) {
        // A busca de várias linhas sempre roda sobre as linhas unidas por `\n`, para que `\n` case o fim
        // de linha de modelos LF e CRLF; `get_multiline_match_range` compensa os deslocamentos.
        let text = model.get_value_in_range_with(range, EndOfLinePreference::Lf);
        let lf_counter = (model.get_eol() == Eol::CrLf).then(|| LineFeedCounter::new(text.units()));
        (text, lf_counter)
    }

    fn do_find_matches_multiline(
        model: &TextModel,
        search_range: &Range,
        searcher: &mut Searcher<'_>,
        capture_matches: bool,
        limit_result_count: usize,
    ) -> Vec<FindMatch> {
        let delta_offset = model.get_offset_at(search_range.get_start_position());
        let (text, lf_counter) = Self::multiline_text(model, search_range);
        let text = text.units();

        let mut result = Vec::new();
        searcher.reset(0);
        while let Some(m) = searcher.next(text) {
            let range = Self::get_multiline_match_range(model, delta_offset, lf_counter.as_ref(), m.start(), m.end() - m.start());
            result.push(create_find_match(range, text, &m, capture_matches));
            if result.len() >= limit_result_count {
                return result;
            }
        }

        result
    }

    fn do_find_matches_line_by_line(
        model: &TextModel,
        search_range: &Range,
        search_data: &SearchData,
        capture_matches: bool,
        limit_result_count: usize,
    ) -> Vec<FindMatch> {
        let mut result = Vec::new();
        let start_column = i64::from(search_range.start_column);
        let end_column = i64::from(search_range.end_column);

        // Caso rápido de um intervalo que começa e termina na mesma linha.
        if search_range.start_line_number == search_range.end_line_number {
            let line = model.get_line_content(search_range.start_line_number);
            let text = js_string::substring(line.units(), start_column - 1, end_column - 1);
            Self::find_matches_in_line(
                search_data,
                text,
                search_range.start_line_number,
                search_range.start_column - 1,
                &mut result,
                capture_matches,
                limit_result_count,
            );
            return result;
        }

        // Os resultados da primeira linha.
        let line = model.get_line_content(search_range.start_line_number);
        let text = js_string::substring(line.units(), start_column - 1, line.len() as i64);
        Self::find_matches_in_line(
            search_data,
            text,
            search_range.start_line_number,
            search_range.start_column - 1,
            &mut result,
            capture_matches,
            limit_result_count,
        );

        // Os resultados das linhas do meio.
        let mut line_number = search_range.start_line_number + 1;
        while line_number < search_range.end_line_number && result.len() < limit_result_count {
            let line = model.get_line_content(line_number);
            Self::find_matches_in_line(
                search_data,
                line.units(),
                line_number,
                0,
                &mut result,
                capture_matches,
                limit_result_count,
            );
            line_number += 1;
        }

        // Os resultados da última linha.
        if result.len() < limit_result_count {
            let line = model.get_line_content(search_range.end_line_number);
            let text = js_string::substring(line.units(), 0, end_column - 1);
            Self::find_matches_in_line(
                search_data,
                text,
                search_range.end_line_number,
                0,
                &mut result,
                capture_matches,
                limit_result_count,
            );
        }

        result
    }

    /// `_findMatchesInLine`: o `resultLen` do original é `result.len()`.
    fn find_matches_in_line(
        search_data: &SearchData,
        text: &[u16],
        line_number: i32,
        delta_offset: i32,
        result: &mut Vec<FindMatch>,
        capture_matches: bool,
        limit_result_count: usize,
    ) {
        let word_separators = search_data.word_separators.as_deref();
        if !capture_matches && let Some(search_string) = &search_data.simple_search {
            let search_string = search_string.units();
            let search_string_len = search_string.len();

            let mut from = 0;
            while let Some(last_match_index) = js_string::index_of(text, search_string, from) {
                from = last_match_index + search_string_len;
                if word_separators.is_none_or(|separators| is_valid_match(separators, text, last_match_index, search_string_len)) {
                    let start_column = last_match_index as i32 + 1 + delta_offset;
                    result.push(FindMatch {
                        range: Range::new(line_number, start_column, line_number, start_column + search_string_len as i32),
                        matches: None,
                    });
                    if result.len() >= limit_result_count {
                        return;
                    }
                }
            }
            return;
        }

        let mut searcher = Searcher::new(word_separators, &search_data.regex);
        // Recomeça a regex do início.
        searcher.reset(0);
        while let Some(m) = searcher.next(text) {
            result.push(create_find_match(
                Self::line_match_range(line_number, delta_offset, &m),
                text,
                &m,
                capture_matches,
            ));
            if result.len() >= limit_result_count {
                return;
            }
        }
    }

    /// O intervalo de um casamento dentro de uma linha, com o texto da linha começando em `delta_offset`.
    fn line_match_range(line_number: i32, delta_offset: i32, m: &Match) -> Range {
        let start_column = m.start() as i32 + 1 + delta_offset;
        Range::new(line_number, start_column, line_number, start_column + (m.end() - m.start()) as i32)
    }

    /// `findNextMatch(model, searchParams, searchStart, captureMatches)`.
    pub fn find_next_match(
        model: &TextModel,
        search_params: &SearchParams,
        search_start: Position,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        let search_data = search_params.parse_search_request()?;

        let mut searcher = Searcher::new(search_data.word_separators.as_deref(), &search_data.regex);

        if search_data.regex.flags().multiline {
            return Self::do_find_next_match_multiline(model, search_start, &mut searcher, capture_matches);
        }
        Self::do_find_next_match_line_by_line(model, search_start, &mut searcher, capture_matches)
    }

    fn do_find_next_match_multiline(
        model: &TextModel,
        search_start: Position,
        searcher: &mut Searcher<'_>,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        let search_text_start = Position::new(search_start.line_number, 1);
        let delta_offset = model.get_offset_at(search_text_start);
        let line_count = model.get_line_count();
        let (text, lf_counter) = Self::multiline_text(
            model,
            &Range::new(
                search_text_start.line_number,
                search_text_start.column,
                line_count,
                model.get_line_max_column(line_count),
            ),
        );
        let text = text.units();
        searcher.reset((search_start.column - 1) as usize);
        if let Some(m) = searcher.next(text) {
            let range = Self::get_multiline_match_range(model, delta_offset, lf_counter.as_ref(), m.start(), m.end() - m.start());
            return Some(create_find_match(range, text, &m, capture_matches));
        }

        if search_start.line_number != 1 || search_start.column != 1 {
            // Tenta de novo a partir do topo.
            return Self::do_find_next_match_multiline(model, Position::new(1, 1), searcher, capture_matches);
        }

        None
    }

    fn do_find_next_match_line_by_line(
        model: &TextModel,
        search_start: Position,
        searcher: &mut Searcher<'_>,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        let line_count = model.get_line_count();
        let start_line_number = search_start.line_number;

        // Procura na primeira linha.
        let text = model.get_line_content(start_line_number);
        if let Some(r) = Self::find_first_match_in_line(searcher, text.units(), start_line_number, search_start.column, capture_matches) {
            return Some(r);
        }

        for i in 1..=line_count {
            let line_index = (start_line_number + i - 1) % line_count;
            let text = model.get_line_content(line_index + 1);
            if let Some(r) = Self::find_first_match_in_line(searcher, text.units(), line_index + 1, 1, capture_matches) {
                return Some(r);
            }
        }

        None
    }

    fn find_first_match_in_line(
        searcher: &mut Searcher<'_>,
        text: &[u16],
        line_number: i32,
        from_column: i32,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        // Põe a regex para procurar a partir da coluna.
        searcher.reset((from_column - 1) as usize);
        let m = searcher.next(text)?;
        Some(create_find_match(
            Self::line_match_range(line_number, 0, &m),
            text,
            &m,
            capture_matches,
        ))
    }

    /// `findPreviousMatch(model, searchParams, searchStart, captureMatches)`.
    pub fn find_previous_match(
        model: &TextModel,
        search_params: &SearchParams,
        search_start: Position,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        let search_data = search_params.parse_search_request()?;

        let mut searcher = Searcher::new(search_data.word_separators.as_deref(), &search_data.regex);

        if search_data.regex.flags().multiline {
            return Self::do_find_previous_match_multiline(model, search_start, &mut searcher, capture_matches);
        }
        Self::do_find_previous_match_line_by_line(model, search_start, &mut searcher, capture_matches)
    }

    fn do_find_previous_match_multiline(
        model: &TextModel,
        search_start: Position,
        searcher: &mut Searcher<'_>,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        let matches = Self::do_find_matches_multiline(
            model,
            &Range::new(1, 1, search_start.line_number, search_start.column),
            searcher,
            capture_matches,
            10 * LIMIT_FIND_COUNT,
        );
        if let Some(last) = matches.into_iter().next_back() {
            return Some(last);
        }

        let line_count = model.get_line_count();
        if search_start.line_number != line_count || search_start.column != model.get_line_max_column(line_count) {
            // Tenta de novo com todo o conteúdo.
            return Self::do_find_previous_match_multiline(
                model,
                Position::new(line_count, model.get_line_max_column(line_count)),
                searcher,
                capture_matches,
            );
        }

        None
    }

    fn do_find_previous_match_line_by_line(
        model: &TextModel,
        search_start: Position,
        searcher: &mut Searcher<'_>,
        capture_matches: bool,
    ) -> Option<FindMatch> {
        let line_count = model.get_line_count();
        let start_line_number = search_start.line_number;

        // Procura na primeira linha.
        let line = model.get_line_content(start_line_number);
        let text = js_string::substring(line.units(), 0, i64::from(search_start.column) - 1);
        if let Some(r) = Self::find_last_match_in_line(searcher, text, start_line_number, capture_matches) {
            return Some(r);
        }

        for i in 1..=line_count {
            let line_index = (line_count + start_line_number - i - 1) % line_count;
            let text = model.get_line_content(line_index + 1);
            if let Some(r) = Self::find_last_match_in_line(searcher, text.units(), line_index + 1, capture_matches) {
                return Some(r);
            }
        }

        None
    }

    fn find_last_match_in_line(searcher: &mut Searcher<'_>, text: &[u16], line_number: i32, capture_matches: bool) -> Option<FindMatch> {
        let mut best_result = None;
        searcher.reset(0);
        while let Some(m) = searcher.next(text) {
            best_result = Some(create_find_match(
                Self::line_match_range(line_number, 0, &m),
                text,
                &m,
                capture_matches,
            ));
        }
        best_result
    }
}
