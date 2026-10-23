//! Porte parcial de `src/vs/workbench/services/search/common/search.ts`: o que a prévia de resultados de texto,
//! o casamento de padrão de arquivo e o `ReplacePattern` usam (`IPatternInfo`, `INotebookPatternInfo`,
//! `ITextSearchPreviewOptions`, `ISearchRange`, `SearchRange`, `OneLineRange`, `SearchRangeSetPairing`,
//! `ITextSearchMatch`, `TextSearchMatch`, `IRawFileMatch` e `isFilePatternMatch`).
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - os números do JavaScript (linhas, colunas, contagens) são `i64`, e o texto é [`JsString`]: colunas e
//!   comprimentos contam unidades UTF-16, como no JS (o padrão de busca do usuário pode carregar surrogates
//!   isolados).
//! - os campos opcionais do TypeScript (`?:`) viram `Option`.
//! - `ISearchRange` (interface) é o próprio [`SearchRange`]: só a classe o implementa aqui.
//! - `OneLineRange extends SearchRange` vira [`OneLineRange::new`], que devolve um [`SearchRange`] com as
//!   duas linhas iguais (a subclasse não acrescenta campo nem método).
//! - `ITextSearchMatch` (interface) é o trait [`ITextSearchMatch`], implementado por [`TextSearchMatch`]; o
//!   `uri` e o `cellFragment` opcionais da interface não existem em `TextSearchMatch` e não são portados.
//! - `ranges: ISearchRange | ISearchRange[]` vira `&[SearchRange]` (um intervalo único é uma fatia de um
//!   elemento). Uma lista vazia dá pânico, como o `ranges[0].startLineNumber` do original dá `TypeError`.
//! - os argumentos opcionais viram `Option` (`previewOptions`, `webviewIndex`) ou `bool` (`ignoreCase`, onde
//!   `undefined` e `false` são equivalentes).

use je_base::glob::{self, GlobOptions};
use je_base::js_string::JsString;
use je_base::strings::{fuzzy_contains, get_n_lines};

/// `IPatternInfo`: o padrão de busca e as opções que a caixa de busca do usuário liga e desliga.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PatternInfo {
    pub pattern: JsString,
    pub is_reg_exp: Option<bool>,
    pub is_word_match: Option<bool>,
    pub word_separators: Option<JsString>,
    pub is_multiline: Option<bool>,
    pub is_unicode: Option<bool>,
    pub is_case_sensitive: Option<bool>,
    pub notebook_info: Option<NotebookPatternInfo>,
}

impl PatternInfo {
    /// Um `{ pattern }` sem nenhuma opção.
    pub fn new(pattern: impl Into<JsString>) -> Self {
        Self { pattern: pattern.into(), ..Self::default() }
    }
}

/// `INotebookPatternInfo`: onde, dentro de um notebook, a busca vale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NotebookPatternInfo {
    pub is_in_notebook_markdown_input: Option<bool>,
    pub is_in_notebook_markdown_preview: Option<bool>,
    pub is_in_notebook_cell_input: Option<bool>,
    pub is_in_notebook_cell_output: Option<bool>,
}


// Warning do original: este padrão é usado no editor de busca para detectar offsets. Se mudar, mude também a
// extensão embutida `search-result`.
const SEARCH_ELIDED_PREFIX: &str = "⟪ ";
const SEARCH_ELIDED_SUFFIX: &str = " characters skipped ⟫";

fn utf16_len(text: &str) -> i64 {
    text.encode_utf16().count() as i64
}

/// `SEARCH_ELIDED_MIN_LEN`.
fn search_elided_min_len() -> i64 {
    (utf16_len(SEARCH_ELIDED_PREFIX) + utf16_len(SEARCH_ELIDED_SUFFIX) + 5) * 2
}

/// `DEFAULT_TEXT_SEARCH_PREVIEW_OPTIONS`.
pub const DEFAULT_TEXT_SEARCH_PREVIEW_OPTIONS: ITextSearchPreviewOptions =
    ITextSearchPreviewOptions { match_lines: 100, chars_per_line: 10000 };

/// `ITextSearchPreviewOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ITextSearchPreviewOptions {
    pub match_lines: i64,
    pub chars_per_line: i64,
}

/// `SearchRange` (e `ISearchRange`): linhas e colunas do original, na base que o chamador usar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchRange {
    pub start_line_number: i64,
    pub start_column: i64,
    pub end_line_number: i64,
    pub end_column: i64,
}

/// `ISearchRange`.
pub type ISearchRange = SearchRange;

impl SearchRange {
    pub fn new(start_line_number: i64, start_column: i64, end_line_number: i64, end_column: i64) -> Self {
        Self { start_line_number, start_column, end_line_number, end_column }
    }
}

/// `OneLineRange`: um [`SearchRange`] dentro de uma única linha.
pub struct OneLineRange;

impl OneLineRange {
    pub fn new(line_number: i64, start_column: i64, end_column: i64) -> SearchRange {
        SearchRange::new(line_number, start_column, line_number, end_column)
    }
}

/// `SearchRangeSetPairing`: o intervalo na fonte e o intervalo correspondente na prévia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchRangeSetPairing {
    pub source: ISearchRange,
    pub preview: ISearchRange,
}

/// `ITextSearchMatch`.
pub trait ITextSearchMatch {
    fn range_locations(&self) -> &[SearchRangeSetPairing];
    fn preview_text(&self) -> &JsString;
    fn webview_index(&self) -> Option<usize>;
}

/// `TextSearchMatch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchMatch {
    pub range_locations: Vec<SearchRangeSetPairing>,
    pub preview_text: JsString,
    pub webview_index: Option<usize>,
}

impl ITextSearchMatch for TextSearchMatch {
    fn range_locations(&self) -> &[SearchRangeSetPairing] {
        &self.range_locations
    }

    fn preview_text(&self) -> &JsString {
        &self.preview_text
    }

    fn webview_index(&self) -> Option<usize> {
        self.webview_index
    }
}

/// `text.slice(start, end)` do JavaScript sobre unidades UTF-16.
fn slice(text: &[u16], start: i64, end: i64) -> &[u16] {
    let len = text.len() as i64;
    let resolve = |index: i64| if index < 0 { (len + index).max(0) } else { index.min(len) };
    let (start, end) = (resolve(start), resolve(end));
    if start >= end { &[] } else { &text[start as usize..end as usize] }
}

impl TextSearchMatch {
    pub fn new(
        text: impl Into<JsString>,
        ranges: &[ISearchRange],
        preview_options: Option<&ITextSearchPreviewOptions>,
        webview_index: Option<usize>,
    ) -> Self {
        let text: JsString = text.into();
        let mut range_locations = Vec::new();

        // Apara a prévia se for um só casamento de linha única com prévia pedida. Do contrário manda o texto
        // inteiro, como para substituir ou para mostrar várias prévias.
        // TODO do original: "this is fishy".
        let preview_text = match preview_options {
            Some(options) if options.match_lines == 1 && is_single_line_range_list(ranges) => {
                // prévia de 1 linha pedida
                let text = get_n_lines(text.units(), options.match_lines as usize);

                let mut result: Vec<u16> = Vec::new();
                let mut shift: i64 = 0;
                let mut last_end: i64 = 0;
                let leading_chars = options.chars_per_line.div_euclid(5);
                for range in ranges {
                    let preview_start = (range.start_column - leading_chars).max(0);
                    let preview_end = range.start_column + options.chars_per_line;
                    if preview_start > last_end + leading_chars + search_elided_min_len() {
                        let elision: Vec<u16> =
                            format!("{SEARCH_ELIDED_PREFIX}{}{SEARCH_ELIDED_SUFFIX}", preview_start - last_end)
                                .encode_utf16()
                                .collect();
                        result.extend_from_slice(&elision);
                        result.extend_from_slice(slice(text, preview_start, preview_end));
                        shift += preview_start - (last_end + elision.len() as i64);
                    } else {
                        result.extend_from_slice(slice(text, last_end, preview_end));
                    }

                    last_end = preview_end;
                    range_locations.push(SearchRangeSetPairing {
                        source: *range,
                        preview: OneLineRange::new(0, range.start_column - shift, range.end_column - shift),
                    });
                }

                JsString::from_units(result)
            }
            _ => {
                let first_match_line = ranges[0].start_line_number;

                range_locations = ranges
                    .iter()
                    .map(|r| SearchRangeSetPairing {
                        preview: SearchRange::new(
                            r.start_line_number - first_match_line,
                            r.start_column,
                            r.end_line_number - first_match_line,
                            r.end_column,
                        ),
                        source: *r,
                    })
                    .collect();
                text
            }
        };

        Self { range_locations, preview_text, webview_index }
    }
}

fn is_single_line_range_list(ranges: &[ISearchRange]) -> bool {
    let line = ranges[0].start_line_number;
    ranges.iter().all(|r| r.start_line_number == line && r.end_line_number == line)
}

/// `IRawFileMatch`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IRawFileMatch {
    pub base: Option<String>,
    /// O caminho do arquivo relativo à pasta `base` que o contém, exatamente como aparece no sistema de
    /// arquivos.
    pub relative_path: String,
    /// O caminho transformado para a busca (por exemplo, o `relative_path` com o nome da pasta do workspace
    /// na frente, para a busca casar também o nome da pasta). Sem valor (ou vazio), a busca usa o
    /// `relative_path`.
    pub search_path: Option<String>,
}

/// `isFilePatternMatch`: `fuzzy` usa `fuzzyContains`; senão o padrão é um glob, que ignora a caixa se
/// `ignore_case`.
pub fn is_file_pattern_match(candidate: &IRawFileMatch, file_pattern_to_use: &str, fuzzy: bool, ignore_case: bool) -> bool {
    let path_to_match = match candidate.search_path.as_deref() {
        Some(search_path) if !search_path.is_empty() => search_path,
        _ => candidate.relative_path.as_str(),
    };
    if fuzzy {
        let target: Vec<u16> = path_to_match.encode_utf16().collect();
        let query: Vec<u16> = file_pattern_to_use.encode_utf16().collect();
        fuzzy_contains(&target, &query)
    } else {
        // `filePatternIgnoreCaseOptions` é `{ ignoreCase: true }`; sem `ignoreCase` o original passa `undefined`.
        let options = if ignore_case {
            GlobOptions { ignore_case: Some(true), ..GlobOptions::default() }
        } else {
            GlobOptions::default()
        };
        glob::match_pattern(file_pattern_to_use, path_to_match, &options)
    }
}
