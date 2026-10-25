//! Porte de `src/vs/workbench/services/search/node/ripgrepTextSearchEngine.ts`: o parser da saída JSON do ripgrep
//! (`RipgrepParser`), a montagem dos argumentos (`getRgArgs`), os
//! ajustes de padrão (`fixRegexNewline`, `fixNewline`, `unicodeEscapesToPCRE2`), a expansão de chaves
//! (`performBraceExpansionForRipgrep`), os tipos da mensagem (`IRgMessage`, `IRgMatch`, `IRgSubmatch`,
//! `IRgBytesOrText`) e, no fim do arquivo, o `RipgrepTextSearchEngine`, que lança o `rg` e trata o stderr (com
//! `SearchError`), mais `rgErrorMsgForDisplay`.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `RipgrepParser extends EventEmitter` guarda ouvintes (`on_result`, `on_hit_limit`) em vez de herdar.
//! - `handleData(Buffer | string)` é [`RgData`]; o `StringDecoder` do Node é o [`Utf8Decoder`] daqui (sequência
//!   incompleta no fim de um bloco espera o próximo, byte inválido vira U+FFFD).
//! - o JSON da linha é lido como `serde_json::Value` (só `match` e `context` têm os campos lidos, como no
//!   original) e vira [`IRgMessage`]. Linha que não é JSON dá pânico com `malformed line from rg`.
//! - `fixRegexNewline` usa o `vscode-regexpp` no original (AST do padrão em modo sem `u`). Aqui o padrão é
//!   validado pelo YARR (`zyarr`) e um percurso próprio, com a mesma pilha de contexto do visitante, emite os
//!   mesmos eventos (`Character` `\n`, `Quantifier`, `CharacterClass`, `CharacterClassRange`, lookbehind).
//! - `getRgArgs` muda `query` (`pattern`, `isRegExp`), como o original, e devolve os argumentos como `String`
//!   (o `rg` os recebe em UTF-8; surrogate isolado vira U+FFFD).
//! - o `RegExp.source` do original escapa `/` e as quebras de linha; aqui o mesmo é reproduzido por
//!   [`rg_regexp_source`], já com o `\/` desfeito que o original faz.

use je_base::buffer::decode_base64;
use je_base::collections::group_by;
use je_base::errors::{self, Error};
use je_base::glob::split_glob_aware;
use je_base::js_regexp::{match_all, replace_all};
use je_base::js_string::JsString;
use je_base::path::{Environment, ProcessEnvironment};
use je_base::strings::{RegExpOptions, create_reg_exp, escape_reg_exp_characters};
use je_base::uri::URI;
use serde_json::{Value, json};

use super::ripgrep_search_utils::{anchor_glob, range_to_search_range, search_range_to_range};
use super::search::{ITextSearchPreviewOptions, TextSearchMatch};
use super::search_ext_types::{
    GlobPattern, Range, TextSearchContext2, TextSearchMatch2, TextSearchMatchRanges, TextSearchQuery2,
    TextSearchResult2,
};
use super::search_ext_types_internal::RipgrepTextSearchOptions;

// ---- mensagens do ripgrep --------------------------------------------------------------------------

/// `IRgBytesOrText`: `{ bytes: string }` (base64) ou `{ text: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IRgBytesOrText {
    Bytes(String),
    Text(String),
}

impl IRgBytesOrText {
    fn from_json(value: &Value) -> Self {
        // `obj.bytes ? ... : obj.text`: `bytes` vazio é falso.
        match value.get("bytes").and_then(Value::as_str) {
            Some(bytes) if !bytes.is_empty() => IRgBytesOrText::Bytes(bytes.to_owned()),
            _ => IRgBytesOrText::Text(value.get("text").and_then(Value::as_str).unwrap_or_default().to_owned()),
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            IRgBytesOrText::Bytes(bytes) => json!({ "bytes": bytes }),
            IRgBytesOrText::Text(text) => json!({ "text": text }),
        }
    }
}

/// `IRgSubmatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IRgSubmatch {
    pub match_: IRgBytesOrText,
    pub start: i64,
    pub end: i64,
}

/// `IRgMatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IRgMatch {
    pub path: IRgBytesOrText,
    pub lines: IRgBytesOrText,
    pub line_number: i64,
    pub absolute_offset: i64,
    pub submatches: Vec<IRgSubmatch>,
}

impl IRgMatch {
    fn from_json(value: &Value) -> Self {
        let number = |key: &str| value.get(key).and_then(Value::as_i64).unwrap_or_default();
        IRgMatch {
            path: IRgBytesOrText::from_json(&value["path"]),
            lines: IRgBytesOrText::from_json(&value["lines"]),
            line_number: number("line_number"),
            absolute_offset: number("absolute_offset"),
            submatches: value
                .get("submatches")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .map(|m| IRgSubmatch {
                            match_: IRgBytesOrText::from_json(&m["match"]),
                            start: m.get("start").and_then(Value::as_i64).unwrap_or_default(),
                            end: m.get("end").and_then(Value::as_i64).unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path.to_json(),
            "lines": self.lines.to_json(),
            "line_number": self.line_number,
            "absolute_offset": self.absolute_offset,
            "submatches": self.submatches.iter().map(|m| json!({
                "match": m.match_.to_json(),
                "start": m.start,
                "end": m.end,
            })).collect::<Vec<_>>(),
        })
    }
}

/// `IRgMessage`: `type` é `'match' | 'context' | string`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IRgMessage {
    pub message_type: String,
    pub data: IRgMatch,
}

impl IRgMessage {
    pub fn to_json(&self) -> Value {
        json!({ "type": self.message_type, "data": self.data.to_json() })
    }
}

// ---- decodificador UTF-8 (StringDecoder) -----------------------------------------------------------

/// O `StringDecoder` do Node para UTF-8.
#[derive(Debug, Default)]
pub struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// `write`: decodifica o que for completo e guarda o resto de uma sequência incompleta.
    pub fn write(&mut self, data: &[u8]) -> String {
        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend_from_slice(data);
        let mut out = String::new();
        let mut rest: &[u8] = &bytes;
        loop {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    out.push_str(valid);
                    break;
                }
                Err(error) => {
                    let (valid, after) = rest.split_at(error.valid_up_to());
                    out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                    match error.error_len() {
                        Some(len) => {
                            out.push('\u{FFFD}');
                            rest = &after[len..];
                        }
                        None => {
                            self.pending = after.to_vec();
                            break;
                        }
                    }
                }
            }
        }
        out
    }

    /// `end`: o que sobrou de uma sequência incompleta vira U+FFFD.
    pub fn end(&mut self) -> String {
        if std::mem::take(&mut self.pending).is_empty() { String::new() } else { "\u{FFFD}".to_owned() }
    }
}

// ---- parser ----------------------------------------------------------------------------------------

/// `Buffer | string` de `handleData`.
pub enum RgData<'a> {
    Bytes(&'a [u8]),
    Text(&'a str),
}

impl<'a> From<&'a str> for RgData<'a> {
    fn from(value: &'a str) -> Self {
        RgData::Text(value)
    }
}

impl<'a> From<&'a [u8]> for RgData<'a> {
    fn from(value: &'a [u8]) -> Self {
        RgData::Bytes(value)
    }
}

/// `String.prototype.trim`: os espaços do JavaScript (`WhiteSpace` e `LineTerminator`).
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{D}'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

/// `bytesOrTextToString`.
fn bytes_or_text_to_string(obj: &IRgBytesOrText) -> String {
    match obj {
        IRgBytesOrText::Bytes(bytes) => String::from_utf8_lossy(&decode_base64(bytes).to_vec()).into_owned(),
        IRgBytesOrText::Text(text) => text.clone(),
    }
}

/// `getNumLinesAndLastNewlineLength`: o número de `\n` e o comprimento (em unidades UTF-16) depois do último.
fn get_num_lines_and_last_newline_length(text: &JsString) -> (i64, i64) {
    let newline = u16::from(b'\n');
    let mut num_lines = 0;
    let mut last_newline_idx: i64 = -1;
    for (index, &unit) in text.units().iter().enumerate() {
        if unit == newline {
            num_lines += 1;
            last_newline_idx = index as i64;
        }
    }
    let last_line_length =
        if last_newline_idx >= 0 { text.len() as i64 - last_newline_idx - 1 } else { text.len() as i64 };
    (num_lines, last_line_length)
}

/// `Buffer#slice(start, end)`: limita aos extremos; `start >= end` dá vazio.
fn slice_bytes(bytes: &[u8], start: i64, end: i64) -> &[u8] {
    let clamp = |index: i64| usize::try_from(index.max(0)).map_or(bytes.len(), |i| i.min(bytes.len()));
    let (start, end) = (clamp(start), clamp(end));
    if start >= end { &[] } else { &bytes[start..end] }
}

/// `RipgrepParser`.
pub struct RipgrepParser {
    remainder: String,
    is_done: bool,
    hit_limit: bool,
    string_decoder: Utf8Decoder,
    num_results: i64,
    max_results: i64,
    root: URI,
    preview_options: ITextSearchPreviewOptions,
    result_listeners: Vec<Box<dyn FnMut(&TextSearchResult2)>>,
    hit_limit_listeners: Vec<Box<dyn FnMut()>>,
}

impl RipgrepParser {
    pub fn new(max_results: i64, root: URI, preview_options: ITextSearchPreviewOptions) -> Self {
        Self {
            remainder: String::new(),
            is_done: false,
            hit_limit: false,
            string_decoder: Utf8Decoder::new(),
            num_results: 0,
            max_results,
            root,
            preview_options,
            result_listeners: Vec::new(),
            hit_limit_listeners: Vec::new(),
        }
    }

    /// `on('result', listener)`.
    pub fn on_result(&mut self, listener: impl FnMut(&TextSearchResult2) + 'static) -> &mut Self {
        self.result_listeners.push(Box::new(listener));
        self
    }

    /// `on('hitLimit', listener)`.
    pub fn on_hit_limit(&mut self, listener: impl FnMut() + 'static) -> &mut Self {
        self.hit_limit_listeners.push(Box::new(listener));
        self
    }

    pub fn cancel(&mut self) {
        self.is_done = true;
    }

    pub fn flush(&mut self) {
        let rest = self.string_decoder.end();
        self.handle_decoded_data(&rest);
    }

    pub fn handle_data<'a>(&mut self, data: impl Into<RgData<'a>>) {
        if self.is_done {
            return;
        }

        let data_str = match data.into() {
            RgData::Text(text) => text.to_owned(),
            RgData::Bytes(bytes) => self.string_decoder.write(bytes),
        };
        self.handle_decoded_data(&data_str);
    }

    fn handle_decoded_data(&mut self, decoded_data: &str) {
        // Se o bloco anterior não terminou em quebra de linha, ele vai na frente deste.
        let data_str = format!("{}{}", self.remainder, decoded_data);

        let Some(first_newline) = decoded_data.find('\n') else {
            // Atalho
            self.remainder = data_str;
            return;
        };
        let mut newline_idx = Some(first_newline + self.remainder.len());

        let mut prev_idx = 0;
        while let Some(idx) = newline_idx {
            self.handle_line(data_str[prev_idx..idx].trim_matches(is_js_whitespace));
            prev_idx = idx + 1;
            newline_idx = data_str[prev_idx..].find('\n').map(|found| found + prev_idx);
        }

        self.remainder = data_str[prev_idx..].to_owned();
    }

    fn handle_line(&mut self, output_line: &str) {
        if self.is_done || output_line.is_empty() {
            return;
        }

        let parsed_line: Value = serde_json::from_str(output_line)
            .unwrap_or_else(|_| errors::throw(Error::new(format!("malformed line from rg: {output_line}"))));

        match parsed_line.get("type").and_then(Value::as_str) {
            Some("match") => {
                let data = IRgMatch::from_json(&parsed_line["data"]);
                let match_path = bytes_or_text_to_string(&data.path);
                let uri = URI::join_path(&self.root, &[match_path.as_str()], ProcessEnvironment.platform());
                let result = self.create_text_search_match(&data, uri);
                self.on_result_emit(TextSearchResult2::Match(result));

                if self.hit_limit {
                    self.cancel();
                    for listener in &mut self.hit_limit_listeners {
                        listener();
                    }
                }
            }
            Some("context") => {
                let data = IRgMatch::from_json(&parsed_line["data"]);
                let context_path = bytes_or_text_to_string(&data.path);
                let uri = URI::join_path(&self.root, &[context_path.as_str()], ProcessEnvironment.platform());
                for result in self.create_text_search_contexts(&data, &uri) {
                    self.on_result_emit(TextSearchResult2::Context(result));
                }
            }
            _ => {}
        }
    }

    fn create_text_search_match(&mut self, data: &IRgMatch, uri: URI) -> TextSearchMatch2 {
        let line_number = data.line_number - 1;
        let full_text = bytes_or_text_to_string(&data.lines);
        let full_text_js = JsString::from(full_text.as_str());
        let full_text_bytes = full_text.as_bytes();

        let mut prev_match_end: i64 = 0;
        let mut prev_match_end_col: i64 = 0;
        let mut prev_match_end_line = line_number;

        // Certas regexes casam a linha, mas fazem o rg não emitir nenhum submatch dela.
        // https://github.com/microsoft/vscode/issues/100569#issuecomment-738496991
        let mut submatches: Vec<(i64, i64, JsString)> = data
            .submatches
            .iter()
            .map(|m| (m.start, m.end, JsString::from(bytes_or_text_to_string(&m.match_))))
            .collect();
        if submatches.is_empty() {
            submatches.push(match full_text_js.units().first() {
                Some(&first) => (0, 1, JsString::from_units(vec![first])),
                None => (0, 0, JsString::new()),
            });
        }

        let mut ranges: Vec<Range> = Vec::new();
        for (start, end, match_text) in &submatches {
            if self.hit_limit {
                continue;
            }

            self.num_results += 1;
            if self.num_results >= self.max_results {
                // Termina a linha, depois relata o resultado abaixo
                self.hit_limit = true;
            }

            let in_between_text = JsString::from(
                String::from_utf8_lossy(slice_bytes(full_text_bytes, prev_match_end, *start)).as_ref(),
            );
            let (in_between_num_lines, in_between_last_line_length) =
                get_num_lines_and_last_newline_length(&in_between_text);
            let start_col = if in_between_num_lines > 0 {
                in_between_last_line_length
            } else {
                in_between_last_line_length + prev_match_end_col
            };

            let (num_lines, last_line_length) = get_num_lines_and_last_newline_length(match_text);
            let start_line_number = in_between_num_lines + prev_match_end_line;
            let end_line_number = num_lines + start_line_number;
            let end_col = if num_lines > 0 { last_line_length } else { last_line_length + start_col };

            prev_match_end = *end;
            prev_match_end_col = end_col;
            prev_match_end_line = end_line_number;

            ranges.push(Range::new(start_line_number, start_col, end_line_number, end_col));
        }

        let search_ranges: Vec<_> = ranges.iter().map(range_to_search_range).collect();

        let internal_result = TextSearchMatch::new(full_text_js, &search_ranges, Some(&self.preview_options), None);
        TextSearchMatch2::new(
            uri,
            internal_result
                .range_locations
                .iter()
                .map(|e| TextSearchMatchRanges {
                    source_range: search_range_to_range(&e.source),
                    preview_range: search_range_to_range(&e.preview),
                })
                .collect(),
            internal_result.preview_text,
        )
    }

    fn create_text_search_contexts(&self, data: &IRgMatch, uri: &URI) -> Vec<TextSearchContext2> {
        let text = bytes_or_text_to_string(&data.lines);
        let start_line = data.line_number;
        // `.replace(/\r?\n$/, '')`
        let text = text.strip_suffix("\r\n").or_else(|| text.strip_suffix('\n')).unwrap_or(&text);
        text.split('\n')
            .enumerate()
            .map(|(i, line)| TextSearchContext2::new(uri.clone(), line, start_line + i as i64))
            .collect()
    }

    fn on_result_emit(&mut self, result: TextSearchResult2) {
        for listener in &mut self.result_listeners {
            listener(&result);
        }
    }
}

// ---- argumentos do rg ------------------------------------------------------------------------------

/// `args.push('-g', glob)`.
fn push_glob(args: &mut Vec<String>, glob: impl Into<String>) {
    args.push("-g".to_owned());
    args.push(glob.into());
}

/// O `RegExp.source` que o original passa ao `rg`: `/` e quebras de linha escapados pelo JavaScript, com o
/// `\/` já desfeito (`.replace(/\\\//g, '/')`).
fn rg_regexp_source(source: &[u16]) -> String {
    let (backslash, slash) = (u16::from(b'\\'), u16::from(b'/'));
    let mut out: Vec<u16> = Vec::with_capacity(source.len());
    let mut i = 0;
    while i < source.len() {
        let unit = source[i];
        if unit == backslash && source.get(i + 1) == Some(&slash) {
            out.push(slash);
            i += 2;
        } else if unit == backslash && i + 1 < source.len() {
            out.extend_from_slice(&source[i..i + 2]);
            i += 2;
        } else {
            match unit {
                0x0A => out.extend("\\n".encode_utf16()),
                0x0D => out.extend("\\r".encode_utf16()),
                0x2028 => out.extend("\\u2028".encode_utf16()),
                0x2029 => out.extend("\\u2029".encode_utf16()),
                _ => out.push(unit),
            }
            i += 1;
        }
    }
    String::from_utf16_lossy(&out)
}

/// `getRgArgs` (exportada para teste no original).
pub fn get_rg_args(query: &mut TextSearchQuery2, options: &RipgrepTextSearchOptions) -> Vec<String> {
    let folder_options = &options.folder_options;
    let mut args: Vec<String> = vec!["--hidden".to_owned(), "--no-require-git".to_owned()];
    args.push(if query.is_case_sensitive == Some(true) { "--case-sensitive" } else { "--ignore-case" }.to_owned());

    if folder_options.ignore_glob_case == Some(true) {
        args.push("--glob-case-insensitive".to_owned());
        args.push("--ignore-file-case-insensitive".to_owned());
    }

    let groups = group_by(&folder_options.includes, |include| {
        if include.starts_with("**") { "doubleStarIncludes" } else { "otherIncludes" }
    });
    let group = |name: &str| groups.iter().find(|(key, _)| *key == name).map(|(_, list)| list.as_slice());
    let (double_star_includes, other_includes) = (group("doubleStarIncludes"), group("otherIncludes"));

    if let Some(other_includes) = other_includes.filter(|list| !list.is_empty()) {
        let mut unique_others: Vec<&String> = Vec::new();
        for other in other_includes {
            if !unique_others.contains(&other) {
                unique_others.push(other);
            }
        }

        push_glob(&mut args, "!*");
        for other_include in unique_others {
            for glob_arg in spread_glob_components(other_include) {
                push_glob(&mut args, anchor_glob(&glob_arg));
            }
        }
    }

    if let Some(double_star_includes) = double_star_includes.filter(|list| !list.is_empty()) {
        for glob_arg in double_star_includes {
            push_glob(&mut args, glob_arg.clone());
        }
    }

    for exclude in &folder_options.excludes {
        let pattern = match exclude {
            GlobPattern::Pattern(pattern) => pattern,
            GlobPattern::Relative(relative) => &relative.pattern,
        };
        push_glob(&mut args, format!("!{}", anchor_glob(pattern)));
    }

    if let Some(max_file_size) = options.max_file_size.filter(|&size| size != 0) {
        args.push("--max-filesize".to_owned());
        args.push(max_file_size.to_string());
    }

    if folder_options.use_ignore_files.local {
        if !folder_options.use_ignore_files.parent {
            args.push("--no-ignore-parent".to_owned());
        }
    } else {
        // Não usa .gitignore nem .ignore
        args.push("--no-ignore".to_owned());
    }

    if folder_options.follow_symlinks {
        args.push("--follow".to_owned());
    }

    if !folder_options.encoding.is_empty() && folder_options.encoding != "utf8" {
        args.push("--encoding".to_owned());
        args.push(folder_options.encoding.clone());
    }

    if let Some(num_threads) = options.num_threads.filter(|&threads| threads != 0) {
        args.push("--threads".to_owned());
        args.push(num_threads.to_string());
    }

    // O ripgrep trata -- como separador de argumentos. Só --. - é ok, --- é ok, --some-flag também. Precisa de
    // caso especial.
    if query.pattern == "--" {
        query.is_reg_exp = Some(true);
        query.pattern = JsString::from("\\-\\-");
    }

    if query.is_multiline == Some(true) && query.is_reg_exp != Some(true) {
        query.pattern = escape_reg_exp_characters(query.pattern.units());
        query.is_reg_exp = Some(true);
    }

    // Deixa o $ casar /r/n
    args.push("--crlf".to_owned());

    if query.is_reg_exp == Some(true) {
        query.pattern = unicode_escapes_to_pcre2(query.pattern.units());
        args.push("--engine".to_owned());
        args.push("auto".to_owned());
    }

    let mut search_pattern_after_double_dashes: Option<String> = None;
    if query.is_word_match == Some(true) {
        let regexp = create_reg_exp(
            query.pattern.units(),
            query.is_reg_exp == Some(true),
            &RegExpOptions { whole_word: true, ..RegExpOptions::default() },
        )
        .unwrap_or_else(|error| errors::throw(Error::new(error.to_string())));
        args.push("--regexp".to_owned());
        args.push(rg_regexp_source(regexp.source()));
    } else if query.is_reg_exp == Some(true) {
        let fixed_regexp_query = fix_newline(fix_regex_newline(query.pattern.units()).units());
        args.push("--regexp".to_owned());
        args.push(fixed_regexp_query.to_string_lossy());
    } else {
        search_pattern_after_double_dashes = Some(query.pattern.to_string_lossy());
        args.push("--fixed-strings".to_owned());
    }

    args.push("--no-config".to_owned());
    if !folder_options.use_ignore_files.global {
        args.push("--no-ignore-global".to_owned());
    }

    args.push("--json".to_owned());

    if query.is_multiline == Some(true) {
        args.push("--multiline".to_owned());
    }

    if let Some(surrounding_context) = options.surrounding_context.filter(|&context| context != 0) {
        args.push("--before-context".to_owned());
        args.push(surrounding_context.to_string());
        args.push("--after-context".to_owned());
        args.push(surrounding_context.to_string());
    }

    // Pasta a buscar
    args.push("--".to_owned());

    if let Some(pattern) = search_pattern_after_double_dashes.filter(|pattern| !pattern.is_empty()) {
        // Põe a consulta depois de --, caso ela comece com traço
        args.push(pattern);
    }

    args.push(".".to_owned());

    args
}

/// `"foo/*bar/something"` -> `["foo", "foo/*bar", "foo/*bar/something", "foo/*bar/something/**"]`
fn spread_glob_components(glob_component: &str) -> Vec<String> {
    perform_brace_expansion_for_ripgrep(glob_component)
        .iter()
        .flat_map(|glob_arg| {
            let components = split_glob_aware(glob_arg, '/');
            (0..components.len()).map(|i| components[..=i].join("/")).collect::<Vec<_>>()
        })
        .collect()
}

// ---- ajustes de padrão -----------------------------------------------------------------------------

/// `unicodeEscapesToPCRE2`.
pub fn unicode_escapes_to_pcre2(pattern: &[u16]) -> JsString {
    // Casa ሴ e, depois, \u{1234}. \u com 5 a 6 caracteres fica como está, porque \x só leva 4.
    let patterns = [
        r"((?:[^\\]|^)(?:\\\\)*)\\u([a-z0-9]{4})",
        r"((?:[^\\]|^)(?:\\\\)*)\\u\{([a-z0-9]{4})\}",
    ];
    let mut pattern = JsString::from(pattern);
    for source in patterns {
        let regexp = zyarr::RegExp::from_str(source, "gi").expect("o padrão fixo é válido");
        while !match_all(&regexp, pattern.units()).is_empty() {
            pattern = replace_all(&regexp, pattern.units(), |captures| {
                let mut replacement = JsString::new();
                replacement.push_units(captures.group(1).unwrap_or_default());
                replacement.push_units(&"\\x{".encode_utf16().collect::<Vec<_>>());
                replacement.push_units(captures.group(2).unwrap_or_default());
                replacement.push_unit(u16::from(b'}'));
                replacement
            });
        }
    }
    pattern
}

/// `fixNewline`: `\n` literal vira `\r?\n`.
pub fn fix_newline(pattern: &[u16]) -> JsString {
    let replacement: Vec<u16> = "\\r?\\n".encode_utf16().collect();
    let mut out = JsString::new();
    for &unit in pattern {
        if unit == u16::from(b'\n') {
            out.push_units(&replacement);
        } else {
            out.push_unit(unit);
        }
    }
    out
}

/// O contexto da pilha do visitante de `fixRegexNewline` (`context: ReAST.Node[]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
    Quantifier,
    CharacterClassRange,
    /// `start` e `end` são os índices do `[` e logo depois do `]`; `in_quantifier` é `parent?.type === 'Quantifier'`.
    CharacterClass { negate: bool, start: usize, end: usize, in_quantifier: bool },
    /// Uma asserção lookbehind, com um id para o `context[0] === node` do `onAssertionLeave`.
    Lookbehind(u32),
}

/// O percurso de `fixRegexNewline`: o visitante do original e o parser que lhe dá os eventos.
struct NewlineFixer<'a> {
    pattern: &'a [u16],
    pos: usize,
    output: Vec<u16>,
    last_emitted_index: usize,
    context: Vec<Context>,
    next_lookbehind_id: u32,
}

fn unit(c: char) -> u16 {
    c as u16
}

impl<'a> NewlineFixer<'a> {
    fn get(&self, index: usize) -> Option<u16> {
        self.pattern.get(index).copied()
    }

    /// `pattern.slice(start, end)`.
    fn slice(&self, start: usize, end: usize) -> &'a [u16] {
        let end = end.min(self.pattern.len());
        if start >= end { &[] } else { &self.pattern[start..end] }
    }

    /// `replace(start, end, text)` do original.
    fn replace(&mut self, start: usize, end: usize, text: &[u16]) {
        let skipped = self.slice(self.last_emitted_index, start);
        self.output.extend_from_slice(skipped);
        self.output.extend_from_slice(text);
        self.last_emitted_index = end;
    }

    fn replace_str(&mut self, start: usize, end: usize, text: &str) {
        let text: Vec<u16> = text.encode_utf16().collect();
        self.replace(start, end, &text);
    }

    /// `onCharacterEnter` para um `\n` em `[start, end)`.
    fn on_newline_character(&mut self, start: usize, end: usize) {
        let Some(parent) = self.context.first().copied() else {
            // caractere simples, \n -> \r?\n
            self.replace_str(start, end, "\\r?\\n");
            return;
        };
        if self.context.iter().any(|node| matches!(node, Context::Lookbehind(_))) {
            // sem efeito dentro de um lookbehind, ver #100569
            return;
        }
        match parent {
            Context::CharacterClass { negate: true, start: class_start, end: class_end, in_quantifier } => {
                // expressão negativa, [^a-z\n] -> (?![a-z]|\r?\n)
                let mut other_content = self.slice(class_start + 2, start).to_vec();
                other_content.extend_from_slice(self.slice(end, class_end - 1));
                let empty = other_content.is_empty();
                if in_quantifier {
                    // Com quantificador não dá para usar lookahead negativo. Mas o `.` já não casa quebra de
                    // linha, então vale ele (com as outras negações).
                    let (prefix, suffix) = if empty { (".", "") } else { ("[^", "]") };
                    self.replace_wrapped(class_start, class_end, prefix, &other_content, suffix);
                } else {
                    let (prefix, suffix) = if empty { ("(?!\\r?\\n", ")") } else { ("(?!\\r?\\n|[", "])") };
                    self.replace_wrapped(class_start, class_end, prefix, &other_content, suffix);
                }
            }
            Context::CharacterClass { negate: false, start: class_start, end: class_end, .. } => {
                // expressão positiva, [a-z\n] -> (?:[a-z]|\r?\n)
                let mut other_content = self.slice(class_start + 1, start).to_vec();
                other_content.extend_from_slice(self.slice(end, class_end - 1));
                let (prefix, suffix) = if other_content.is_empty() { ("\\r?\\n", "") } else { ("(?:[", "]|\\r?\\n)") };
                self.replace_wrapped(class_start, class_end, prefix, &other_content, suffix);
            }
            Context::Quantifier => self.replace_str(start, end, "(?:\\r?\\n)"),
            Context::CharacterClassRange | Context::Lookbehind(_) => {}
        }
    }

    /// `replace` com o texto `prefixo + miolo + sufixo`; o miolo vai em unidades UTF-16 (pode ter surrogate
    /// isolado, que `String` não carrega).
    fn replace_wrapped(&mut self, start: usize, end: usize, prefix: &str, content: &[u16], suffix: &str) {
        let mut units: Vec<u16> = prefix.encode_utf16().collect();
        units.extend_from_slice(content);
        units.extend(suffix.encode_utf16());
        self.replace(start, end, &units);
    }

    /// Fim (exclusivo) da classe que começa em `start`.
    fn find_class_end(&self, start: usize) -> usize {
        let mut i = start + 1;
        if self.get(i) == Some(unit('^')) {
            i += 1;
        }
        while let Some(c) = self.get(i) {
            if c == unit('\\') {
                i += 2;
            } else if c == unit(']') {
                return i + 1;
            } else {
                i += 1;
            }
        }
        self.pattern.len()
    }

    /// Fim (exclusivo) do grupo que começa em `start`.
    fn find_group_end(&self, start: usize) -> usize {
        let mut depth = 0;
        let mut i = start;
        while let Some(c) = self.get(i) {
            if c == unit('\\') {
                i += 2;
            } else if c == unit('[') {
                i = self.find_class_end(i);
            } else if c == unit('(') {
                depth += 1;
                i += 1;
            } else if c == unit(')') {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return i;
                }
            } else {
                i += 1;
            }
        }
        self.pattern.len()
    }

    /// O comprimento do cabeçalho do grupo em `start` (`(`, `(?:`, `(?=`, `(?<name>`...).
    fn group_header_len(&self, start: usize) -> usize {
        if self.get(start + 1) != Some(unit('?')) {
            return 1;
        }
        match self.get(start + 2) {
            Some(c) if c == unit(':') || c == unit('=') || c == unit('!') => 3,
            Some(c) if c == unit('<') => match self.get(start + 3) {
                Some(d) if d == unit('=') || d == unit('!') => 4,
                _ => self.find_unit(start + 3, unit('>')) + 1 - start,
            },
            // grupo de modificadores, `(?i:`
            _ => self.find_unit(start + 2, unit(':')) + 1 - start,
        }
    }

    fn find_unit(&self, from: usize, target: u16) -> usize {
        (from..self.pattern.len()).find(|&i| self.pattern[i] == target).unwrap_or(self.pattern.len())
    }

    /// O comprimento do quantificador em `at` (com o `?` preguiçoso), se houver um.
    fn quantifier_len(&self, at: usize) -> Option<usize> {
        let base = match self.get(at)? {
            c if c == unit('*') || c == unit('+') || c == unit('?') => 1,
            c if c == unit('{') => {
                let is_digit = |i: usize| self.get(i).is_some_and(|d| (unit('0')..=unit('9')).contains(&d));
                let mut j = at + 1;
                if !is_digit(j) {
                    return None;
                }
                while is_digit(j) {
                    j += 1;
                }
                if self.get(j) == Some(unit(',')) {
                    j += 1;
                    while is_digit(j) {
                        j += 1;
                    }
                }
                if self.get(j) != Some(unit('}')) {
                    return None;
                }
                j + 1 - at
            }
            _ => return None,
        };
        Some(if self.get(at + base) == Some(unit('?')) { base + 1 } else { base })
    }

    /// `Disjunction`: alternativas até o `)` do grupo (sem consumi-lo) ou o fim do padrão.
    fn parse_disjunction(&mut self, in_group: bool) {
        while let Some(c) = self.get(self.pos) {
            if in_group && c == unit(')') {
                break;
            }
            if c == unit('|') {
                self.pos += 1;
            } else {
                self.parse_term();
            }
        }
    }

    fn parse_term(&mut self) {
        let start = self.pos;
        let c = self.pattern[start];
        if c == unit('^') || c == unit('$') {
            self.pos += 1;
            return;
        }
        if c == unit('\\') && matches!(self.get(start + 1), Some(n) if n == unit('b') || n == unit('B')) {
            self.pos += 2;
            return;
        }
        if c == unit('(') && self.get(start + 1) == Some(unit('?')) && self.get(start + 2) == Some(unit('<'))
            && matches!(self.get(start + 3), Some(n) if n == unit('=') || n == unit('!'))
        {
            self.parse_lookbehind(start);
            return;
        }

        let element_end = if c == unit('\\') {
            (start + 2).min(self.pattern.len())
        } else if c == unit('[') {
            self.find_class_end(start)
        } else if c == unit('(') {
            self.find_group_end(start)
        } else {
            start + 1
        };
        let quantifier = self.quantifier_len(element_end);
        if quantifier.is_some() {
            self.context.insert(0, Context::Quantifier);
        }

        if c == unit('\\') {
            if self.get(start + 1) == Some(unit('n')) {
                self.on_newline_character(start, element_end);
            }
        } else if c == unit('[') {
            self.parse_class(start, element_end, quantifier.is_some());
        } else if c == unit('(') {
            self.pos = start + self.group_header_len(start);
            self.parse_disjunction(true);
        }

        self.pos = element_end + quantifier.unwrap_or(0);
        if quantifier.is_some() {
            self.context.remove(0);
        }
    }

    fn parse_lookbehind(&mut self, start: usize) {
        let id = self.next_lookbehind_id;
        self.next_lookbehind_id += 1;
        let node = Context::Lookbehind(id);
        self.context.push(node);
        self.pos = start + 4;
        self.parse_disjunction(true);
        self.pos += 1;
        // `onAssertionLeave`: só sai da pilha se for o primeiro.
        if self.context.first() == Some(&node) {
            self.context.remove(0);
        }
    }

    /// Um átomo de classe: `(início, fim, é_conjunto, é_\n)`. `é_conjunto` vale para `\d \D \s \S \w \W`.
    fn parse_class_atom(&mut self) -> (usize, usize, bool, bool) {
        let start = self.pos;
        if self.pattern[start] == unit('\\') && start + 1 < self.pattern.len() {
            let next = self.pattern[start + 1];
            self.pos = start + 2;
            let is_set = [unit('d'), unit('D'), unit('s'), unit('S'), unit('w'), unit('W')].contains(&next);
            (start, start + 2, is_set, next == unit('n'))
        } else {
            self.pos = start + 1;
            (start, start + 1, false, false)
        }
    }

    fn parse_class(&mut self, start: usize, end: usize, in_quantifier: bool) {
        let negate = self.get(start + 1) == Some(unit('^'));
        self.context.insert(0, Context::CharacterClass { negate, start, end, in_quantifier });
        self.pos = start + if negate { 2 } else { 1 };
        let close = end - 1;
        while self.pos < close {
            let (start1, end1, set1, newline1) = self.parse_class_atom();
            let is_range_dash = self.get(self.pos) == Some(unit('-')) && self.pos + 1 < close;
            if !is_range_dash {
                if newline1 {
                    self.on_newline_character(start1, end1);
                }
                continue;
            }
            let dash = self.pos;
            self.pos += 1;
            let (start2, end2, set2, newline2) = self.parse_class_atom();
            if set1 || set2 {
                // `[\w-a]`: sem `u`, o `-` é um caractere comum e nada vira intervalo.
                for (newline, atom_start, atom_end) in [(newline1, start1, end1), (false, dash, dash + 1), (newline2, start2, end2)] {
                    if newline {
                        self.on_newline_character(atom_start, atom_end);
                    }
                }
            } else {
                self.context.insert(0, Context::CharacterClassRange);
                if newline1 {
                    self.on_newline_character(start1, end1);
                }
                if newline2 {
                    self.on_newline_character(start2, end2);
                }
                self.context.remove(0);
            }
        }
        self.pos = end;
        self.context.remove(0);
    }
}

/// `fixRegexNewline`: faz o `\n` de uma regex casar também o `\r\n`.
pub fn fix_regex_newline(pattern: &[u16]) -> JsString {
    // Parseia o padrão de novo a cada vez; padrão inválido volta como está.
    if zyarr::RegExp::new(pattern, &[]).is_err() {
        return JsString::from(pattern);
    }

    let mut fixer = NewlineFixer {
        pattern,
        pos: 0,
        output: Vec::new(),
        last_emitted_index: 0,
        context: Vec::new(),
        next_lookbehind_id: 0,
    };
    fixer.parse_disjunction(false);
    let rest = fixer.slice(fixer.last_emitted_index, pattern.len());
    fixer.output.extend_from_slice(rest);
    JsString::from_units(fixer.output)
}

// ---- expansão de chaves ----------------------------------------------------------------------------

/// O resultado de `getEscapeAwareSplitStringForRipgrep`.
struct BraceSplit {
    fixed_start: Option<String>,
    str_in_braces: String,
    fixed_end: Option<String>,
}

/// Divide a string na primeira oportunidade de expansão de chaves.
/// - Se a chave vem depois de um `\`, ela está escapada.
/// - Não processa escapes dentro do sub-glob.
/// - Se duas `{` não escapadas aparecem antes de `}`, o ripgrep dá erro de aninhamento, então não divide.
fn get_escape_aware_split_string_for_ripgrep(pattern: &str) -> BraceSplit {
    let mut in_braces = false;
    let mut escaped = false;
    let mut fixed_start = String::new();
    let mut str_in_braces = String::new();
    for (i, ch) in pattern.char_indices() {
        let rest = &pattern[i + ch.len_utf8()..];
        // O texto de dentro ou de fora das chaves, conforme o estado.
        macro_rules! dest {
            () => {
                if in_braces { &mut str_in_braces } else { &mut fixed_start }
            };
        }
        match ch {
            '\\' => {
                if escaped {
                    // Já escapada: deixa a barra escapada e a que a escapa; as duas viram uma barra no
                    // processamento posterior do glob.
                    dest!().push_str("\\\\");
                    escaped = false;
                } else {
                    escaped = true;
                }
            }
            '{' => {
                if escaped {
                    // Chave de abertura escapada é literal: tira a `\`, já reconhecida, e põe a `{`.
                    dest!().push('{');
                    escaped = false;
                } else if in_braces {
                    // O ripgrep vê um grupo alternativo aninhado, que é inválido: devolve o padrão com as
                    // mudanças das chaves escapadas.
                    return BraceSplit {
                        fixed_start: None,
                        str_in_braces: format!("{fixed_start}{{{str_in_braces}{{{rest}"),
                        fixed_end: None,
                    };
                } else {
                    in_braces = true;
                }
            }
            '}' => {
                if escaped {
                    dest!().push('}');
                    escaped = false;
                } else if in_braces {
                    // Achou o fim de uma chave de abertura válida.
                    return BraceSplit {
                        fixed_start: Some(fixed_start),
                        str_in_braces,
                        fixed_end: Some(rest.to_owned()),
                    };
                } else {
                    // Fora das chaves e sem escape é uma `}` literal, ainda em `fixedStart`.
                    fixed_start.push('}');
                }
            }
            _ => {
                // Como no caso da `\\`: o escape não foi usado, então volta para a string que o consumirá.
                let dest = dest!();
                if escaped {
                    dest.push('\\');
                }
                dest.push(ch);
                escaped = false;
            }
        }
    }

    // Não chegou na última chave, então nada se divide.
    let tail = if in_braces { format!("{{{str_in_braces}") } else { String::new() };
    BraceSplit { fixed_start: None, str_in_braces: format!("{fixed_start}{tail}"), fixed_end: None }
}

/// Lê as chaves e devolve os globs equivalentes. Só suporta um nível de aninhamento. Exportada para teste.
pub fn perform_brace_expansion_for_ripgrep(pattern: &str) -> Vec<String> {
    let BraceSplit { fixed_start, str_in_braces, fixed_end } = get_escape_aware_split_string_for_ripgrep(pattern);
    let (Some(fixed_start), Some(fixed_end)) = (fixed_start, fixed_end) else {
        return vec![str_in_braces];
    };

    let mut arr = split_glob_aware(&str_in_braces, ',');

    if arr.is_empty() {
        // ocorre se as chaves estão vazias.
        arr = vec![String::new()];
    }

    let ends = perform_brace_expansion_for_ripgrep(&fixed_end);

    arr.iter()
        .flat_map(|elem| {
            let start = format!("{fixed_start}{elem}");
            ends.iter().map(move |end| format!("{start}{end}"))
        })
        .collect()
}

// ---- o processo rg (RipgrepTextSearchEngine) -------------------------------------------------------------
//
// Diferenças de forma em relação ao original, nenhuma de comportamento:
// - `provideTextSearchResults` é o `TextSearchProvider2::provide_text_search_results` do motor (o `Promise<TextSearchComplete2>`
//   do original é o `ProviderResult` com `Some`). A consulta é compartilhada entre as pastas num `Rc<RefCell<..>>`, porque o
//   `getRgArgs` a altera e o original passa o mesmo objeto a todas as pastas.
// - `cp.spawn(rgDiskPath, args, { cwd })` é o [`ChildProcess`] de `file_walker`; `rgProc.on('error' | 'close')`,
//   `stdout.on('data')` e `stderr.on('data')` chegam como [`ChildEvent`] a um ouvinte só. `await rgDiskPath()` é a chamada
//   síncrona de [`rg_disk_path`].
// - `RipgrepParser extends EventEmitter` guarda os ouvintes dentro do `RefCell` do parser: um `cancel()` pedido de dentro de
//   um ouvinte (o limite de resultados, ou o consumidor cancelando o token ao receber um resultado) não pode reentrar no
//   parser, então fica pendente (`parser_cancel_deferred`) e os resultados seguintes do mesmo bloco são descartados, como o
//   `isDone` do parser faria no original.
// - O ouvinte de `token.onCancellationRequested` é descartado quando o processo fecha (o original o deixa pendurado; depois
//   do `close` o `cancel()` não tem mais efeito).
// - `serializeSearchError(new SearchError(..))` é [`serialize_search_error`]; o `console.error(e)` é `eprintln!`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::process::Command;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::{Promise, Resolver};
use je_platform::progress::progress::{IProgress, Progress};
use je_platform::resources::fs_path;

use super::file_walker::{ChildEvent, ChildProcess};
use super::ripgrep_disk_path::rg_disk_path;
use super::ripgrep_search_utils::{IOutputChannel, MAX_RIPGREP_SYMLINK_LOOPS, RipgrepSymlinkLoopDetector};
use super::search::{DEFAULT_MAX_SEARCH_RESULTS, SearchError, SearchErrorCode, serialize_search_error};
use super::search_ext_conversion_types::new_to_old_preview_options;
use super::search_ext_types::{
    ProviderResult, TextSearchComplete2, TextSearchCompleteMessage2, TextSearchCompleteMessageType, TextSearchProvider2,
    TextSearchProviderOptions,
};

/// O `length` do JavaScript (unidades UTF-16).
fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// O `uri` das duas formas de resultado.
fn text_search_result_uri(result: &TextSearchResult2) -> &URI {
    match result {
        TextSearchResult2::Match(text_match) => &text_match.uri,
        TextSearchResult2::Context(context) => &context.uri,
    }
}

/// O `{ limitHit: e.some(...), message: e.flatMap(...) }` com que o motor e o `RipgrepSearchProvider` juntam o resultado
/// de cada pasta.
pub fn merge_text_search_completes(completes: &[TextSearchComplete2]) -> TextSearchComplete2 {
    let mut complete = TextSearchComplete2 {
        // todo do original: get this to actually check
        limit_hit: Some(completes.iter().any(|complete| complete.limit_hit == Some(true))),
        message: None,
    };
    let messages: Vec<TextSearchCompleteMessage2> = completes.iter().flat_map(|complete| complete.message.clone().unwrap_or_default()).collect();
    if !messages.is_empty() {
        complete.message = Some(messages);
    }
    complete
}

/// O `JSON.stringify({ ...options, folder: options.folderOptions.folder.toString() })` do log: os campos indefinidos somem.
fn options_log_json(options: &RipgrepTextSearchOptions) -> String {
    let folder_options = &options.folder_options;
    let excludes: Vec<Value> = folder_options
        .excludes
        .iter()
        .map(|exclude| match exclude {
            GlobPattern::Pattern(pattern) => json!(pattern),
            GlobPattern::Relative(relative) => json!({ "baseUri": relative.base_uri.to_string(false).to_string_lossy(), "pattern": relative.pattern }),
        })
        .collect();
    let mut folder_object = serde_json::Map::new();
    folder_object.insert("folder".to_owned(), json!(folder_options.folder.to_string(false).to_string_lossy()));
    folder_object.insert("includes".to_owned(), json!(folder_options.includes));
    folder_object.insert("excludes".to_owned(), Value::Array(excludes));
    if let Some(ignore_glob_case) = folder_options.ignore_glob_case {
        folder_object.insert("ignoreGlobCase".to_owned(), json!(ignore_glob_case));
    }
    folder_object.insert("followSymlinks".to_owned(), json!(folder_options.follow_symlinks));
    folder_object.insert(
        "useIgnoreFiles".to_owned(),
        json!({ "local": folder_options.use_ignore_files.local, "parent": folder_options.use_ignore_files.parent, "global": folder_options.use_ignore_files.global }),
    );
    folder_object.insert("encoding".to_owned(), json!(folder_options.encoding));

    let mut object = serde_json::Map::new();
    object.insert("folderOptions".to_owned(), Value::Object(folder_object));
    if let Some(num_threads) = options.num_threads {
        object.insert("numThreads".to_owned(), json!(num_threads));
    }
    if let Some(max_results) = options.max_results {
        object.insert("maxResults".to_owned(), json!(max_results));
    }
    if let Some(preview_options) = options.preview_options {
        object.insert("previewOptions".to_owned(), json!({ "matchLines": preview_options.match_lines, "charsPerLine": preview_options.chars_per_line }));
    }
    if let Some(max_file_size) = options.max_file_size {
        object.insert("maxFileSize".to_owned(), json!(max_file_size));
    }
    if let Some(surrounding_context) = options.surrounding_context {
        object.insert("surroundingContext".to_owned(), json!(surrounding_context));
    }
    object.insert("folder".to_owned(), json!(folder_options.folder.to_string(false).to_string_lossy()));
    Value::Object(object).to_string()
}

/// O estado de uma execução do `rg` numa pasta, compartilhado entre os eventos do processo e o `cancel`.
struct RgRun {
    output_channel: Rc<dyn IOutputChannel>,
    progress: Rc<dyn IProgress<TextSearchResult2>>,
    parser: RefCell<RipgrepParser>,
    parser_cancel_deferred: Cell<bool>,
    proc: RefCell<Option<Rc<ChildProcess>>>,
    got_result: Cell<bool>,
    data_without_result: RefCell<String>,
    got_data: Cell<bool>,
    stderr: RefCell<String>,
    is_done: Cell<bool>,
    limit_hit: Cell<bool>,
    is_retrying: Cell<bool>,
    symlink_loop_detector: RefCell<Option<RipgrepSymlinkLoopDetector>>,
    reported_paths: RefCell<Vec<String>>,
}

impl RgRun {
    fn new(
        output_channel: Rc<dyn IOutputChannel>,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        options: &RipgrepTextSearchOptions,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this: &std::rc::Weak<RgRun>| {
            let mut parser = RipgrepParser::new(
                options.max_results.unwrap_or(DEFAULT_MAX_SEARCH_RESULTS as i64),
                options.folder_options.folder.clone(),
                new_to_old_preview_options(options.preview_options),
            );
            let on_result = this.clone();
            parser.on_result(move |result| {
                if let Some(run) = on_result.upgrade() {
                    run.handle_result(result);
                }
            });
            let on_hit_limit = this.clone();
            parser.on_hit_limit(move || {
                if let Some(run) = on_hit_limit.upgrade() {
                    run.limit_hit.set(true);
                    run.cancel();
                }
            });

            // When ripgrep reports too many symbolic link loops, search the folder again without following
            // symbolic links and skip the results that were already reported
            let symlink_loop_detector = options.folder_options.follow_symlinks.then(RipgrepSymlinkLoopDetector::new);
            RgRun {
                output_channel,
                progress,
                parser: RefCell::new(parser),
                parser_cancel_deferred: Cell::new(false),
                proc: RefCell::new(None),
                got_result: Cell::new(false),
                data_without_result: RefCell::new(String::new()),
                got_data: Cell::new(false),
                stderr: RefCell::new(String::new()),
                is_done: Cell::new(false),
                limit_hit: Cell::new(false),
                is_retrying: Cell::new(false),
                symlink_loop_detector: RefCell::new(symlink_loop_detector),
                reported_paths: RefCell::new(Vec::new()),
            }
        })
    }

    /// O ouvinte de `ripgrepParser.on('result')`.
    fn handle_result(&self, result: &TextSearchResult2) {
        if self.parser_cancel_deferred.get() {
            return;
        }
        self.got_result.set(true);
        self.data_without_result.borrow_mut().clear();
        if self.symlink_loop_detector.borrow().is_some() {
            self.reported_paths.borrow_mut().push(text_search_result_uri(result).path().to_string_lossy());
        }
        self.progress.report(result.clone());
    }

    /// `cancel`.
    fn cancel(&self) {
        self.is_done.set(true);

        if let Some(proc) = self.proc.borrow().as_ref() {
            proc.kill();
        }

        match self.parser.try_borrow_mut() {
            Ok(mut parser) => parser.cancel(),
            Err(_) => self.parser_cancel_deferred.set(true),
        }
    }

    /// `rgProc.stdout.on('data')`.
    fn handle_stdout(&self, data: &[u8]) {
        self.parser.borrow_mut().handle_data(data);
        if !self.got_result.get() {
            self.data_without_result.borrow_mut().push_str(&String::from_utf8_lossy(data));
        }
        self.got_data.set(true);
    }

    /// `rgProc.stderr.on('data')`.
    fn handle_stderr(&self, data: &[u8]) {
        let message = String::from_utf8_lossy(data).into_owned();
        self.output_channel.append_line(&message);

        {
            let mut stderr = self.stderr.borrow_mut();
            if js_length(&stderr) + js_length(&message) < 1_000_000 {
                stderr.push_str(&message);
            }
        }

        let detected_loops = !self.is_done.get() && self.symlink_loop_detector.borrow_mut().as_mut().is_some_and(|detector| detector.handle_stderr(&message));
        if detected_loops {
            self.is_retrying.set(true);
            self.cancel();
        }
    }
}

/// `RipgrepTextSearchEngine`.
#[derive(Clone)]
pub struct RipgrepTextSearchEngine {
    output_channel: Rc<dyn IOutputChannel>,
    num_threads: Option<i64>,
}

impl RipgrepTextSearchEngine {
    pub fn new(output_channel: Rc<dyn IOutputChannel>, num_threads: Option<i64>) -> Self {
        Self { output_channel, num_threads }
    }

    pub fn provide_text_search_results_with_rg_options(
        &self,
        query: &Rc<RefCell<TextSearchQuery2>>,
        options: RipgrepTextSearchOptions,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        token: CancellationToken,
    ) -> Promise<TextSearchComplete2> {
        self.output_channel.append_line(&format!("provideTextSearchResults {}, {}", query.borrow().pattern, options_log_json(&options)));

        if query.borrow().pattern.is_empty() {
            return Promise::resolved(TextSearchComplete2 { limit_hit: Some(false), message: None });
        }

        let resolved_rg_disk_path = rg_disk_path();
        let (engine, query) = (self.clone(), query.clone());
        Promise::with_executor(move |resolver| engine.run_ripgrep(resolver, &query, options, progress, token, &resolved_rg_disk_path))
    }

    /// O corpo do executor de `new Promise((resolve, reject) => ...)`.
    fn run_ripgrep(
        &self,
        resolver: Resolver<TextSearchComplete2>,
        query: &Rc<RefCell<TextSearchQuery2>>,
        options: RipgrepTextSearchOptions,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        token: CancellationToken,
        resolved_rg_disk_path: &str,
    ) {
        let run = RgRun::new(self.output_channel.clone(), progress.clone(), &options);
        let cancel_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        *cancel_listener.borrow_mut() = Some(token.on_cancellation_requested().listen({
            let run = Rc::downgrade(&run);
            move |_| {
                if let Some(run) = run.upgrade() {
                    run.cancel();
                }
            }
        }));

        let extended_options = RipgrepTextSearchOptions { num_threads: self.num_threads, ..options.clone() };
        let rg_args = get_rg_args(&mut query.borrow_mut(), &extended_options);

        let cwd = fs_path(&options.folder_options.folder);

        let escaped_args = rg_args.iter().map(|arg| if arg.starts_with('-') { arg.clone() } else { format!("'{arg}'") }).collect::<Vec<_>>().join(" ");
        self.output_channel.append_line(&format!("{resolved_rg_disk_path} {escaped_args}\n - cwd: {cwd}"));

        let rg_proc = ChildProcess::spawn(Command::new(resolved_rg_disk_path).args(&rg_args).current_dir(&cwd));
        *run.proc.borrow_mut() = Some(rg_proc.clone());

        let (engine, query) = (self.clone(), query.clone());
        rg_proc.listen(move |event| match event {
            ChildEvent::Error(error) => {
                eprintln!("{error}");
                run.output_channel.append_line(&format!("Error: {}", error.message));
                resolver.reject(serialize_search_error(&SearchError::new(error.message.clone(), Some(SearchErrorCode::RgProcessError))));
            }
            ChildEvent::Stdout(data) => run.handle_stdout(data),
            ChildEvent::Stderr(data) => run.handle_stderr(data),
            ChildEvent::Close(_) => {
                if let Some(listener) = cancel_listener.borrow_mut().take() {
                    listener.dispose();
                }
                let complete = TextSearchComplete2 { limit_hit: Some(run.limit_hit.get()), message: None };

                run.output_channel.append_line(if run.got_data.get() { "Got data from stdout" } else { "No data from stdout" });
                run.output_channel.append_line(if run.got_result.get() { "Got result from parser" } else { "No result from parser" });
                let data_without_result = run.data_without_result.borrow().clone();
                if !data_without_result.is_empty() {
                    run.output_channel.append_line(&format!("Got data without result: {data_without_result}"));
                }

                run.output_channel.append_line("");

                if run.is_retrying.get() && !token.is_cancellation_requested() {
                    let reported_paths = run.reported_paths.borrow().clone();
                    resolver.resolve_with(&engine.search_again_without_following_symlinks(&query, options.clone(), progress.clone(), token.clone(), &reported_paths));
                } else if run.is_done.get() {
                    resolver.resolve(complete);
                } else {
                    // Trigger last result
                    run.parser.borrow_mut().flush();
                    *run.proc.borrow_mut() = None;
                    let stderr = run.stderr.borrow().clone();
                    let search_error = if !stderr.is_empty() && !run.got_data.get() { rg_error_msg_for_display(&stderr) } else { None };
                    match search_error {
                        Some(search_error) => resolver.reject(serialize_search_error(&SearchError::new(search_error.message, search_error.code))),
                        None => resolver.resolve(complete),
                    }
                }
            }
        });
    }

    fn search_again_without_following_symlinks(
        &self,
        query: &Rc<RefCell<TextSearchQuery2>>,
        options: RipgrepTextSearchOptions,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        token: CancellationToken,
        reported_paths: &[String],
    ) -> Promise<TextSearchComplete2> {
        let folder = options.folder_options.folder.clone();
        self.output_channel.append_line(&format!(
            "ripgrep reported more than {MAX_RIPGREP_SYMLINK_LOOPS} symbolic link loops, searching {} again without following symbolic links",
            fs_path(&folder)
        ));

        // ripgrep reports the results of a file in the same order every time, so skip as many results of each
        // file as were already reported. This also covers a file whose results were only partially reported.
        let mut counts: HashMap<String, usize> = HashMap::new();
        for path in reported_paths {
            *counts.entry(path.clone()).or_insert(0) += 1;
        }
        let results_to_skip = Rc::new(RefCell::new(counts));

        let skipping_progress = Progress::new({
            let progress = progress.clone();
            move |result: TextSearchResult2| {
                let path = text_search_result_uri(&result).path().to_string_lossy();
                let skipped = match results_to_skip.borrow_mut().get_mut(&path) {
                    Some(to_skip) if *to_skip > 0 => {
                        *to_skip -= 1;
                        true
                    }
                    _ => false,
                };
                if !skipped {
                    progress.report(result);
                }
            }
        });

        let mut retry_options = options;
        retry_options.folder_options.follow_symlinks = false;
        self.provide_text_search_results_with_rg_options(query, retry_options, Rc::new(skipping_progress), token).then(move |complete| {
            let mut message = complete.message.unwrap_or_default();
            message.push(TextSearchCompleteMessage2 {
                text: localize(
                    "symlinkLoops",
                    "Symbolic links in '{0}' were not followed because they form too many loops, so results may be incomplete. Exclude the folders that contain the loops or turn off the '{1}' setting to avoid this.",
                    &[fs_path(&folder).as_str(), "search.followSymlinks"],
                ),
                trusted: None,
                message_type: TextSearchCompleteMessageType::Warning,
            });
            TextSearchComplete2 { limit_hit: complete.limit_hit, message: Some(message) }
        })
    }
}

impl TextSearchProvider2 for RipgrepTextSearchEngine {
    fn provide_text_search_results(
        &self,
        query: &TextSearchQuery2,
        options: &TextSearchProviderOptions,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete2> {
        let query = Rc::new(RefCell::new(query.clone()));
        let completes = options
            .folder_options
            .iter()
            .map(|folder_option| {
                let extended_options = RipgrepTextSearchOptions {
                    folder_options: folder_option.clone(),
                    num_threads: self.num_threads,
                    max_results: Some(options.max_results),
                    preview_options: Some(options.preview_options),
                    max_file_size: options.max_file_size,
                    surrounding_context: Some(options.surrounding_context),
                };
                self.provide_text_search_results_with_rg_options(&query, extended_options, progress.clone(), token.clone())
            })
            .collect();
        Promise::all(completes).then(|completes| Some(merge_text_search_completes(&completes)))
    }
}

/// Read the first line of stderr and return an error for display or undefined, based on a list of
/// allowed properties.
/// Ripgrep produces stderr output which is not from a fatal error, and we only want the search to be
/// "failed" when a fatal error was produced.
fn rg_error_msg_for_display(msg: &str) -> Option<SearchError> {
    let lines: Vec<&str> = msg.split('\n').collect();
    let first_line = lines[0].trim_matches(is_js_whitespace);

    if lines.iter().any(|line| line.starts_with("regex parse error")) {
        return Some(SearchError::new(build_regex_parse_error(&lines), Some(SearchErrorCode::RegexParseError)));
    }

    const UNKNOWN_ENCODING: &str = "grep config error: unknown encoding: ";
    if let Some(position) = first_line.find(UNKNOWN_ENCODING) {
        return Some(SearchError::new(format!("Unknown encoding: {}", &first_line[position + UNKNOWN_ENCODING.len()..]), Some(SearchErrorCode::UnknownEncoding)));
    }

    // Uppercase first letter
    let capitalized = || {
        let mut chars = first_line.chars();
        chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
    };

    if first_line.starts_with("error parsing glob") {
        return Some(SearchError::new(capitalized(), Some(SearchErrorCode::GlobParseError)));
    }

    if first_line.starts_with("the literal") {
        return Some(SearchError::new(capitalized(), Some(SearchErrorCode::InvalidLiteral)));
    }

    if first_line.starts_with("PCRE2: error compiling pattern") {
        return Some(SearchError::new(first_line, Some(SearchErrorCode::RegexParseError)));
    }

    None
}

fn build_regex_parse_error(lines: &[&str]) -> String {
    let mut error_message: Vec<String> = vec!["Regex parse error".to_owned()];
    if let Some(pcre2_error_line) = lines.iter().find(|line| line.starts_with("PCRE2:")) {
        let pcre2_error_message = pcre2_error_line.replacen("PCRE2:", "", 1);
        if pcre2_error_message.contains(':')
            && let Some(pcre2_actual_error_message) = pcre2_error_message.split(':').nth(1)
        {
            error_message.push(format!(":{pcre2_actual_error_message}"));
        }
    }

    error_message.join("")
}
