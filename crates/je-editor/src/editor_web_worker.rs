//! Porte parcial de `src/vs/editor/common/services/editorWebWorker.ts` e de `MirrorModel`
//! (`textModelSync/textModelSync.impl.ts`): o `EditorWorker` com o espelho dos modelos (`$acceptNewModel`,
//! `$acceptModelChanged`) e o `$computeWordRanges`, que o teste de `WordDistance` usa no lugar do web worker de
//! verdade. As outras operações do worker (diferenças, edições de espaço em branco, links, cores,
//! `navigateValueSet`, palavras para sugestão) entram junto das contribuições que as usam.
//!
//! Representação:
//! - O worker roda no mesmo thread: os métodos `$...` do original (chamadas de IPC que devolvem `Promise`) são métodos
//!   comuns que recebem os dados já como valores do porte; `IRawModelData` é o [`RawModelData`].
//! - `MirrorModel extends MirrorTextModel` é a composição [`MirrorModel`] sobre o [`MirrorTextModel`]. Dele só entram o
//!   `getLineWords` e o `_wordenize` que o `$computeWordRanges` usa, mais `getValue` e `getLinesContent` (o worker de
//!   notebook, `contrib/notebook/common/services/notebookWebWorker.ts`, os lê).
//! - `$computeWordRanges(modelUrl, range, wordDef, wordDefFlags)` recebe a definição de palavra como o texto e as flags
//!   do `RegExp` (o IPC não leva o objeto) e devolve a promessa de [`WordRanges`]; `Object.create(null)` é um
//!   `HashMap`. `!isNaN(Number(word.word))` é [`is_numeric_word`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use je_base::JsString;
use je_base::diff::string_diff;
use je_base::errors::{Error, throw};
use je_base::js_regexp::JsRegExp;
use je_base::js_string::{substring, trim};
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;
use je_base::strings::split_lines;
use je_base::uri::URI;

use crate::languages::ILink;
use crate::link_computer::{ILinkComputerTarget, compute_links};
use crate::code_action::languages::TextEdit;
use crate::diff::default_lines_diff_computer::default_lines_diff_computer::DefaultLinesDiffComputer;
use crate::diff::document_diff_provider::{DocumentDiff, DocumentDiffProviderOptions};
use crate::diff::legacy_lines_diff_computer::{DiffComputer, DiffComputerOpts, LegacyLinesDiffComputer, LineChange};
use crate::diff::lines_diff_computer::{ILinesDiffComputer, LinesDiff, LinesDiffComputerOptions};
use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::edits::string_edit::StringEdit;
use crate::editor_worker::{DiffAlgorithmName, WordRanges};
use crate::inplace_replace_support::{InplaceReplaceSupportResult, navigate_value_set};
use crate::mirror_text_model::MirrorTextModel;
use crate::model::Eol;
use crate::position::Position;
use crate::position_to_offset::PositionOffsetTransformerBase;
use crate::range::Range;
use crate::text::abstract_text::{AbstractText, StringText};
use crate::text_model::TextModel;
use crate::text_model_events::ModelContentChangedEvent;
use crate::word_helper::{WordAtPosition, ensure_valid_word_definition, get_word_at_text};

/// `resolveLinesDiffComputer(algorithm)` e `.computeDiff(...)`: o `linesDiffComputers` do algoritmo pedido.
fn compute_lines_diff(algorithm: DiffAlgorithmName, original_lines: &[JsString], modified_lines: &[JsString], options: &LinesDiffComputerOptions) -> LinesDiff {
    match algorithm {
        DiffAlgorithmName::Legacy => LegacyLinesDiffComputer.compute_diff(original_lines, modified_lines, options),
        DiffAlgorithmName::Advanced | DiffAlgorithmName::AdvancedExternal | DiffAlgorithmName::AdvancedWasm => {
            DefaultLinesDiffComputer::new().compute_diff(original_lines, modified_lines, options)
        }
    }
}

/// `computeStringDiff(original, modified, options, algorithm)` (`@internal` em `editorWebWorker.ts`): a
/// diferença de linhas dos dois textos, refinada por caractere, como uma `StringEdit` sobre o original.
pub fn compute_string_diff(original: &JsString, modified: &JsString, max_computation_time_ms: f64, algorithm: DiffAlgorithmName) -> StringEdit {
    let original_text = StringText::new(original.clone());
    let original_lines = original_text.get_lines();
    let modified_text = StringText::new(modified.clone());
    let modified_lines = modified_text.get_lines();

    let options = LinesDiffComputerOptions {
        ignore_trim_whitespace: false,
        max_computation_time_ms,
        compute_moves: false,
        extend_to_subwords: Some(false),
    };
    let result = compute_lines_diff(algorithm, &original_lines, &modified_lines, &options);

    let text_edit = DetailedLineRangeMapping::to_text_edit(&result.changes, &modified_text);
    original_text.get_transformer().get_string_edit(&text_edit)
}

/// `IRawModelData`: o modelo que o worker passa a espelhar.
pub struct RawModelData {
    pub url: String,
    pub lines: Vec<JsString>,
    pub eol: Eol,
    pub version_id: i64,
}

/// `MirrorModel`: o espelho de um modelo no worker.
pub struct MirrorModel {
    inner: MirrorTextModel,
}

impl MirrorModel {
    pub fn new(uri: URI, lines: Vec<JsString>, eol: Eol, version_id: i64) -> Self {
        Self { inner: MirrorTextModel::new(Some(uri), lines, eol, version_id) }
    }

    /// `getValue()`: o texto inteiro, com o fim de linha do espelho.
    pub fn get_value(&self) -> JsString {
        self.inner.get_text()
    }

    /// `getLinesContent()`: uma cópia das linhas.
    pub fn get_lines_content(&self) -> Vec<JsString> {
        self.inner.lines().to_vec()
    }

    /// `onEvents(e)`.
    pub fn on_events(&mut self, e: &ModelContentChangedEvent) {
        self.inner.on_events(e);
    }

    /// `getLineWords(lineNumber, wordDefinition)`: as palavras da linha, com as colunas em que começam e terminam.
    pub fn get_line_words(&self, line_number: i32, word_definition: &JsRegExp) -> Vec<WordAtPosition> {
        let content = self.inner.lines()[(line_number - 1) as usize].units();
        Self::wordenize(content, word_definition)
            .into_iter()
            .map(|(start, end)| WordAtPosition {
                word: JsString::from(substring(content, start as i64, end as i64)),
                start_column: start as i32 + 1,
                end_column: end as i32 + 1,
            })
            .collect()
    }

    /// `uri`.
    pub fn uri(&self) -> &URI {
        self.inner.uri().expect("o espelho do worker sempre nasce com uri")
    }

    /// `eol`.
    pub fn eol(&self) -> Eol {
        self.inner.eol()
    }

    /// `getLineCount()`.
    pub fn get_line_count(&self) -> i32 {
        self.inner.lines().len() as i32
    }

    /// `getLineContent(lineNumber)`.
    pub fn get_line_content(&self, line_number: i32) -> JsString {
        self.inner.lines()[(line_number - 1) as usize].clone()
    }

    /// `words(wordDefinition)`: as palavras do modelo, linha a linha, sem materializar as de linhas ainda não lidas.
    pub fn words<'a>(&'a self, word_definition: &'a JsRegExp) -> impl Iterator<Item = JsString> + 'a {
        self.inner.lines().iter().flat_map(move |line| {
            let content = line.units();
            Self::wordenize(content, word_definition)
                .into_iter()
                .map(move |(start, end)| JsString::from(substring(content, start as i64, end as i64)))
        })
    }

    /// `_validatePosition(position)`: a posição trazida para dentro do texto.
    fn validate_position(&self, position: Position) -> Position {
        let lines = self.inner.lines();
        let Position { mut line_number, mut column } = position;
        if line_number < 1 {
            line_number = 1;
            column = 1;
        } else if line_number as usize > lines.len() {
            line_number = lines.len() as i32;
            column = lines[(line_number - 1) as usize].len() as i32 + 1;
        } else {
            let max_character = lines[(line_number - 1) as usize].len() as i32 + 1;
            column = column.clamp(1, max_character);
        }
        Position::new(line_number, column)
    }

    /// `_validateRange(range)`.
    fn validate_range(&self, range: &Range) -> Range {
        let start = self.validate_position(range.get_start_position());
        let end = self.validate_position(range.get_end_position());
        Range { start_line_number: start.line_number, start_column: start.column, end_line_number: end.line_number, end_column: end.column }
    }

    /// `getValueInRange(range)`: o texto do intervalo (validado), com o fim de linha do espelho.
    pub fn get_value_in_range(&self, range: &Range) -> JsString {
        let range = self.validate_range(range);
        let lines = self.inner.lines();
        let start_index = (range.start_line_number - 1) as usize;
        let end_index = (range.end_line_number - 1) as usize;

        if start_index == end_index {
            return JsString::from(substring(lines[start_index].units(), i64::from(range.start_column - 1), i64::from(range.end_column - 1)));
        }

        let first = lines[start_index].units();
        let mut result = substring(first, i64::from(range.start_column - 1), first.len() as i64).to_vec();
        for line in &lines[start_index + 1..end_index] {
            result.extend_from_slice(self.inner.eol().as_units());
            result.extend_from_slice(line.units());
        }
        result.extend_from_slice(self.inner.eol().as_units());
        result.extend_from_slice(substring(lines[end_index].units(), 0, i64::from(range.end_column - 1)));
        JsString::from_units(result)
    }

    /// `offsetAt(position)`.
    pub fn offset_at(&mut self, position: Position) -> usize {
        let position = self.validate_position(position);
        let line_starts = self.inner.ensure_line_starts();
        line_starts.get_prefix_sum(i64::from(position.line_number) - 2) as usize + (position.column - 1) as usize
    }

    /// `positionAt(offset)`: o deslocamento é limitado ao texto.
    pub fn position_at(&mut self, offset: i64) -> Position {
        let offset = offset.max(0);
        let out = self.inner.ensure_line_starts().get_index_of(offset);
        let line_length = self.inner.lines()[out.index].len() as i64;

        // Ensure we return a valid position
        Position::new(1 + out.index as i32, 1 + out.remainder.min(line_length) as i32)
    }

    /// `_wordenize(content, wordDefinition)`: o início e o fim (em unidades UTF-16) de cada palavra.
    fn wordenize(content: &[u16], word_definition: &JsRegExp) -> Vec<(usize, usize)> {
        let mut result = Vec::new();

        word_definition.set_last_index(0); // reset lastIndex just to be sure

        while let Some(found) = word_definition.exec(content) {
            if found.end() == found.start() {
                // it did match the empty string
                break;
            }
            result.push((found.start(), found.end()));
        }
        result
    }
}

/// `!isNaN(Number(word))`: a palavra é um número do JavaScript (`Number('')` é 0, então o texto vazio também é).
pub fn is_numeric_word(word: &[u16]) -> bool {
    let text = String::from_utf16_lossy(trim(word));
    if text.is_empty() {
        return true;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return !digits.is_empty() && digits.chars().all(|digit| digit.is_digit(radix));
        }
    }
    if text.strip_prefix(['+', '-']).unwrap_or(&text) == "Infinity" {
        return true;
    }
    // Só dígitos, ponto, expoente e sinal: o `parse` do Rust também aceitaria `inf` e `nan`.
    text.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) && text.parse::<f64>().is_ok()
}

/// `EditorWorker._diffLimit`.
const DIFF_LIMIT: usize = 100_000;

/// `EditorWorker._suggestionsLimit`.
const SUGGESTIONS_LIMIT: usize = 10_000;

/// O resultado de `$textualSuggest`: as palavras, na ordem em que apareceram, e quanto tempo levou (ms).
#[derive(Clone, Debug, PartialEq)]
pub struct TextualSuggestResult {
    pub words: Vec<JsString>,
    pub duration: f64,
}

impl ILinkComputerTarget for MirrorModel {
    fn get_line_count(&self) -> i32 {
        MirrorModel::get_line_count(self)
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        MirrorModel::get_line_content(self, line_number)
    }
}

/// `text.replace(/\r\n|\n|\r/g, model.eol)`.
fn normalize_eol(text: &str, eol: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                result.push_str(eol);
            }
            '\n' => result.push_str(eol),
            other => result.push(other),
        }
    }
    result
}

/// O que `$computeMoreMinimalEdits` e `$computeHumanReadableDiff` leem do modelo (`ICommonModel` no worker, `ITextModel`
/// no editor): o texto de um intervalo, as conversões entre posição e deslocamento e o fim de linha. Os métodos pedem
/// `&mut` porque o espelho calcula a soma de prefixo das linhas sob demanda.
pub trait EditsModel {
    fn value_in_range(&mut self, range: &Range) -> JsString;
    fn offset_at(&mut self, position: Position) -> usize;
    fn position_at(&mut self, offset: i64) -> Position;
    fn eol_text(&self) -> &'static str;
}

impl EditsModel for TextModel {
    fn value_in_range(&mut self, range: &Range) -> JsString {
        self.get_value_in_range(range)
    }

    fn offset_at(&mut self, position: Position) -> usize {
        self.get_offset_at(position)
    }

    fn position_at(&mut self, offset: i64) -> Position {
        self.get_position_at(offset)
    }

    fn eol_text(&self) -> &'static str {
        match self.get_eol() {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
        }
    }
}

impl EditsModel for MirrorModel {
    fn value_in_range(&mut self, range: &Range) -> JsString {
        self.get_value_in_range(range)
    }

    fn offset_at(&mut self, position: Position) -> usize {
        MirrorModel::offset_at(self, position)
    }

    fn position_at(&mut self, offset: i64) -> Position {
        MirrorModel::position_at(self, offset)
    }

    fn eol_text(&self) -> &'static str {
        match self.eol() {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
        }
    }
}

/// Ordena as edições pelo início, como o `sort` de `$computeMoreMinimalEdits` e de `$computeHumanReadableDiff` (o
/// original manda para o fim as edições sem intervalo, só de fim de linha; aqui todas têm intervalo).
fn sort_edits_by_start(edits: &mut [TextEdit]) {
    edits.sort_by(|a, b| Range::compare_ranges_using_starts(Some(&a.range), Some(&b.range)).cmp(&0));
}

/// `EditorWorker.$computeHumanReadableDiff(modelUrl, edits, options)` sobre o modelo que o worker espelha (mesma
/// convenção de [`compute_more_minimal_edits`] para o `modelUrl`): cada edição vira as mudanças internas da diferença de
/// linhas padrão entre o texto original do intervalo e o novo.
pub fn compute_human_readable_diff(model: &mut impl EditsModel, mut edits: Vec<TextEdit>, options: &LinesDiffComputerOptions) -> Vec<TextEdit> {
    let mut result: Vec<TextEdit> = Vec::new();
    let mut last_eol = None;

    sort_edits_by_start(&mut edits);
    let eol = model.eol_text();
    let eol_units: Vec<u16> = eol.encode_utf16().collect();

    for TextEdit { range, text, eol: edit_eol, .. } in edits {
        if edit_eol.is_some() {
            last_eol = edit_eol;
        }

        if range.is_empty() && text.is_empty() {
            // empty change
            continue;
        }

        let original = model.value_in_range(&range);
        let text = JsString::from(normalize_eol(&text, eol));

        if original == text {
            // noop
            continue;
        }

        // make sure diff won't take too long
        if text.len().max(original.len()) > DIFF_LIMIT {
            result.push(TextEdit { range, text: text.to_string_lossy(), eol: None, insert_as_snippet: None, keep_whitespace: None });
            continue;
        }

        // compute diff between original and edit.text
        let original_lines: Vec<JsString> = split_lines(original.units()).into_iter().map(JsString::from).collect();
        let modified_lines: Vec<JsString> = split_lines(text.units()).into_iter().map(JsString::from).collect();

        let diff = DefaultLinesDiffComputer::new().compute_diff(&original_lines, &modified_lines, options);

        let start = range.get_start_position();

        for change in diff.changes {
            let Some(inner_changes) = change.inner_changes else {
                throw(Error::bug_indicating(Some("The experimental diff algorithm always produces inner changes")));
            };
            for inner in inner_changes {
                result.push(TextEdit {
                    range: Range::from_positions(
                        add_positions(start, inner.original_range.get_start_position()),
                        add_positions(start, inner.original_range.get_end_position()),
                    ),
                    text: JsString::from_units(get_range_text(&modified_lines, &inner.modified_range).join(eol_units.as_slice())).to_string_lossy(),
                    eol: None,
                    insert_as_snippet: None,
                    keep_whitespace: None,
                });
            }
        }
    }

    if last_eol.is_some() {
        result.push(TextEdit { range: Range::new(0, 0, 0, 0), text: String::new(), eol: last_eol, insert_as_snippet: None, keep_whitespace: None });
    }

    result
}

/// `addPositions(pos1, pos2)`.
fn add_positions(pos1: Position, pos2: Position) -> Position {
    Position::new(
        pos1.line_number + pos2.line_number - 1,
        if pos2.line_number == 1 { pos1.column + pos2.column - 1 } else { pos2.column },
    )
}

/// `getText(lines, range)`: as linhas do texto que o intervalo cobre, cortadas nas colunas das pontas.
fn get_range_text(lines: &[JsString], range: &Range) -> Vec<Vec<u16>> {
    let mut result = Vec::new();
    for i in range.start_line_number..=range.end_line_number {
        let line = lines[(i - 1) as usize].units();
        let units = if i == range.start_line_number && i == range.end_line_number {
            substring(line, i64::from(range.start_column - 1), i64::from(range.end_column - 1))
        } else if i == range.start_line_number {
            substring(line, i64::from(range.start_column - 1), line.len() as i64)
        } else if i == range.end_line_number {
            substring(line, 0, i64::from(range.end_column - 1))
        } else {
            line
        };
        result.push(units.to_vec());
    }
    result
}

/// `EditorWorker.$computeMoreMinimalEdits(modelUrl, edits, pretty)` sobre o modelo que o worker espelha. O worker roda no
/// mesmo thread e o espelho é o próprio `model`, então o `modelUrl` e o `if (!model) return edits` não existem: quem chama
/// já resolveu o modelo (o `IEditorWorkerService` que o tem).
///
/// O `eol` de uma edição só de fim de linha (sem texto) é guardado e vira a última edição, com o intervalo zerado.
pub fn compute_more_minimal_edits(model: &mut impl EditsModel, mut edits: Vec<TextEdit>, pretty: bool) -> Vec<TextEdit> {
    let mut result: Vec<TextEdit> = Vec::new();
    let mut last_eol = None;

    sort_edits_by_start(&mut edits);

    // merge adjacent edits
    let mut write_index = 0;
    for read_index in 1..edits.len() {
        if edits[write_index].range.get_end_position() == edits[read_index].range.get_start_position() {
            let range = Range::from_positions(edits[write_index].range.get_start_position(), edits[read_index].range.get_end_position());
            let text = edits[read_index].text.clone();
            edits[write_index].range = range;
            edits[write_index].text.push_str(&text);
        } else {
            write_index += 1;
            edits[write_index] = edits[read_index].clone();
        }
    }
    edits.truncate(write_index + 1);

    let eol = model.eol_text();

    for TextEdit { range, text, eol: edit_eol, .. } in edits {
        if edit_eol.is_some() {
            last_eol = edit_eol;
        }

        if range.is_empty() && text.is_empty() {
            // empty change
            continue;
        }

        let original = model.value_in_range(&range);
        let text = JsString::from(normalize_eol(&text, eol));

        if original == text {
            // noop
            continue;
        }

        // make sure diff won't take too long
        if text.len().max(original.len()) > DIFF_LIMIT {
            result.push(TextEdit { range, text: text.to_string_lossy(), eol: None, insert_as_snippet: None, keep_whitespace: None });
            continue;
        }

        // compute diff between original and edit.text
        let changes = string_diff(original, text.clone(), pretty);
        let edit_offset = model.offset_at(range.get_start_position()) as i64;

        for change in changes {
            let start = model.position_at(edit_offset + i64::from(change.original_start));
            let end = model.position_at(edit_offset + i64::from(change.original_start) + i64::from(change.original_length));
            let new_text = JsString::from(substring(text.units(), i64::from(change.modified_start), i64::from(change.modified_start) + i64::from(change.modified_length)));
            let new_range = Range::new(start.line_number, start.column, end.line_number, end.column);

            if model.value_in_range(&new_range) != new_text {
                result.push(TextEdit { range: new_range, text: new_text.to_string_lossy(), eol: None, insert_as_snippet: None, keep_whitespace: None });
            }
        }
    }

    if last_eol.is_some() {
        result.push(TextEdit { range: Range::new(0, 0, 0, 0), text: String::new(), eol: last_eol, insert_as_snippet: None, keep_whitespace: None });
    }

    result
}

/// `EditorWorker`.
#[derive(Default)]
pub struct EditorWorker {
    models: RefCell<HashMap<String, MirrorModel>>,
}

impl EditorWorker {
    pub fn new() -> Self {
        Self::default()
    }

    /// `$acceptNewModel(data)`.
    pub fn accept_new_model(&self, data: RawModelData) {
        let uri = URI::parse(data.url.as_str(), false);
        self.models.borrow_mut().insert(data.url, MirrorModel::new(uri, data.lines, data.eol, data.version_id));
    }

    /// `$acceptModelChanged(uri, e)`: o modelo precisa já ter sido aceito.
    pub fn accept_model_changed(&self, uri: &str, e: &ModelContentChangedEvent) {
        self.models.borrow_mut().get_mut(uri).expect("o modelo foi aceito pelo worker").on_events(e);
    }

    /// `$computeMoreMinimalEdits(modelUrl, edits, pretty)`: sem o modelo, as edições voltam como vieram.
    pub fn compute_more_minimal_edits(&self, model_url: &str, edits: Vec<TextEdit>, pretty: bool) -> Promise<Vec<TextEdit>> {
        match self.models.borrow_mut().get_mut(model_url) {
            Some(model) => Promise::resolved(compute_more_minimal_edits(model, edits, pretty)),
            None => Promise::resolved(edits),
        }
    }

    /// `$computeHumanReadableDiff(modelUrl, edits, options)`: sem o modelo, as edições voltam como vieram.
    pub fn compute_human_readable_diff(&self, model_url: &str, edits: Vec<TextEdit>, options: &LinesDiffComputerOptions) -> Vec<TextEdit> {
        match self.models.borrow_mut().get_mut(model_url) {
            Some(model) => compute_human_readable_diff(model, edits, options),
            None => edits,
        }
    }

    /// `$computeLinks(modelUrl)`.
    pub fn compute_links(&self, model_url: &str) -> Promise<Option<Vec<ILink>>> {
        let models = self.models.borrow();
        Promise::resolved(models.get(model_url).map(|model| compute_links(Some(model as &dyn ILinkComputerTarget))))
    }

    /// `$computeDiff(originalUrl, modifiedUrl, options, algorithm)`: `None` se o worker não tem um dos modelos. O
    /// resultado é o [`DocumentDiff`] (o `getLineChanges` do original só serializa as mudanças para o IPC).
    pub fn compute_diff(&self, original_url: &str, modified_url: &str, options: &DocumentDiffProviderOptions, algorithm: DiffAlgorithmName) -> Promise<Option<DocumentDiff>> {
        let models = self.models.borrow();
        let (Some(original), Some(modified)) = (models.get(original_url), models.get(modified_url)) else {
            return Promise::resolved(None);
        };

        let original_lines = original.get_lines_content();
        let modified_lines = modified.get_lines_content();
        let result = compute_lines_diff(algorithm, &original_lines, &modified_lines, options);

        // `_modelsAreIdentical`: só olhado quando não há mudanças.
        let identical = result.changes.is_empty() && original_lines == modified_lines;

        Promise::resolved(Some(DocumentDiff { identical, quit_early: result.hit_timeout, changes: result.changes, moves: result.moves }))
    }

    /// `$computeDirtyDiff(originalUrl, modifiedUrl, ignoreTrimWhitespace)`: `None` se o worker não tem um dos modelos.
    pub fn compute_dirty_diff(&self, original_url: &str, modified_url: &str, ignore_trim_whitespace: bool) -> Promise<Option<Vec<LineChange>>> {
        let models = self.models.borrow();
        let (Some(original), Some(modified)) = (models.get(original_url), models.get(modified_url)) else {
            return Promise::resolved(None);
        };

        let diff_computer = DiffComputer::new(
            original.get_lines_content(),
            modified.get_lines_content(),
            DiffComputerOpts {
                should_compute_char_changes: false,
                should_post_process_char_changes: false,
                should_ignore_trim_whitespace: ignore_trim_whitespace,
                should_make_pretty_diff: true,
                max_computation_time: 1000.0,
            },
        );
        Promise::resolved(Some(diff_computer.compute_diff().changes))
    }

    /// `$navigateValueSet(modelUrl, range, up, wordDef, wordDefFlags)`.
    pub fn navigate_value_set(&self, model_url: &str, mut range: Range, up: bool, word_def: &[u16], word_def_flags: &str) -> Promise<Option<InplaceReplaceSupportResult>> {
        let models = self.models.borrow();
        let Some(model) = models.get(model_url) else {
            return Promise::resolved(None);
        };

        let word_def_reg_exp = match JsRegExp::new(word_def, word_def_flags) {
            Ok(reg_exp) => Rc::new(reg_exp),
            Err(error) => return Promise::rejected(Error::named("SyntaxError", error.message())),
        };

        if range.start_column == range.end_column {
            range.end_column += 1;
        }

        let selection_text = model.get_value_in_range(&range);

        let word_at_text = get_word_at_text(
            range.start_column,
            &ensure_valid_word_definition(Some(&word_def_reg_exp)),
            model.get_line_content(range.start_line_number).units(),
            0,
            None,
        );
        let Some(word_at_text) = word_at_text else {
            return Promise::resolved(None);
        };
        let word_range = Range {
            start_line_number: range.start_line_number,
            start_column: word_at_text.start_column,
            end_line_number: range.start_line_number,
            end_column: word_at_text.end_column,
        };
        let word = model.get_value_in_range(&word_range);
        Promise::resolved(navigate_value_set(&range, &selection_text, &word_range, Some(&word), up))
    }

    /// `$textualSuggest(modelUrls, leadingWord, wordDef, wordDefFlags)`.
    pub fn textual_suggest(&self, model_urls: &[String], leading_word: Option<&[u16]>, word_def: &[u16], word_def_flags: &str) -> Promise<Option<TextualSuggestResult>> {
        let sw = StopWatch::new(None);
        let word_def_reg_exp = match JsRegExp::new(word_def, word_def_flags) {
            Ok(reg_exp) => reg_exp,
            Err(error) => return Promise::rejected(Error::named("SyntaxError", error.message())),
        };
        let mut seen: HashSet<JsString> = HashSet::new();
        let mut words: Vec<JsString> = Vec::new();
        let models = self.models.borrow();

        'outer: for url in model_urls {
            let Some(model) = models.get(url) else {
                continue;
            };

            for word in model.words(&word_def_reg_exp) {
                if Some(word.units()) == leading_word || is_numeric_word(word.units()) {
                    continue;
                }
                if seen.insert(word.clone()) {
                    words.push(word);
                }
                if seen.len() > SUGGESTIONS_LIMIT {
                    break 'outer;
                }
            }
        }

        Promise::resolved(Some(TextualSuggestResult { words, duration: sw.elapsed() }))
    }

    /// Dá ao `f` o espelho de `uri` (o `_getModel` do original), se o worker o aceitou.
    pub fn with_model<R>(&self, uri: &str, f: impl FnOnce(&mut MirrorModel) -> R) -> Option<R> {
        self.models.borrow_mut().get_mut(uri).map(f)
    }

    /// `$computeWordRanges(modelUrl, range, wordDef, wordDefFlags)`: as palavras que não são números, de
    /// `range.start_line_number` até a linha anterior a `range.end_line_number`, e os intervalos de cada uma.
    pub fn compute_word_ranges(&self, model_url: &str, range: Range, word_def: &[u16], word_def_flags: &str) -> Promise<WordRanges> {
        let models = self.models.borrow();
        let Some(model) = models.get(model_url) else {
            return Promise::resolved(WordRanges::new());
        };
        let word_def_reg_exp = match JsRegExp::new(word_def, word_def_flags) {
            Ok(reg_exp) => reg_exp,
            Err(error) => return Promise::rejected(Error::named("SyntaxError", error.message())),
        };
        let mut result = WordRanges::new();
        for line in range.start_line_number..range.end_line_number {
            for word in model.get_line_words(line, &word_def_reg_exp) {
                if is_numeric_word(word.word.units()) {
                    continue;
                }
                result.entry(word.word).or_default().push(Range {
                    start_line_number: line,
                    start_column: word.start_column,
                    end_line_number: line,
                    end_column: word.end_column,
                });
            }
        }
        Promise::resolved(result)
    }
}
