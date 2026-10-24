//! Porte parcial de `src/vs/workbench/services/search/common/search.ts`: o que a prévia de resultados de texto,
//! o casamento de padrão de arquivo e o `ReplacePattern` usam (`IPatternInfo`, `INotebookPatternInfo`,
//! `ITextSearchPreviewOptions`, `ISearchRange`, `SearchRange`, `OneLineRange`, `SearchRangeSetPairing`,
//! `ITextSearchMatch`, `TextSearchMatch`, `IRawFileMatch` e `isFilePatternMatch`), mais as consultas que o
//! `searchHelpers` e o motor do ripgrep leem (`IFolderQuery`, `ExcludeGlobPattern`, `ICommonQueryProps`,
//! `ITextQueryProps`, `IAITextQueryProps`, `QueryType`, `ITextSearchQuery`, `ITextSearchContext` e
//! `ITextSearchResult`), mais o que o `QueryBuilder` produz e lê (`IFileQueryProps`, `getExcludes` e
//! `pathIncludedInQuery`). A configuração (`ISearchConfiguration`) é o objeto [`Dynamic`] que o serviço de
//! configuração devolve.
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
//! - `U extends UriComponents = URI` das consultas é o parâmetro de tipo `U` (padrão [`URI`]), sem restrição.
//! - `ITextQueryProps extends ICommonQueryProps` vira a composição: o campo `common`. O discriminante `type`
//!   (sempre `QueryType.Text` ou `QueryType.aiText`) é o próprio tipo da struct.
//! - `ITextSearchResult = ITextSearchMatch | ITextSearchContext` é o enum [`ITextSearchResult`], e o `uri`
//!   opcional de `ITextSearchContext` não existe (como em `TextSearchMatch`).
//! - os argumentos opcionais viram `Option` (`previewOptions`, `webviewIndex`) ou `bool` (`ignoreCase`, onde
//!   `undefined` e `false` são equivalentes).

use je_base::dynamic::{Dynamic, Truthy};
use je_base::extpath;
use je_base::glob::{self, Expression, GlobOptions};
use je_base::js_string::JsString;
use je_base::objects;
use je_base::path::PathModule;
use je_base::strings::{fuzzy_contains, get_n_lines};
use je_base::uri::URI;
use je_platform::resources::{fs_path, host_environment};

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

/// `ExcludeGlobPattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExcludeGlobPattern<U = URI> {
    pub folder: Option<U>,
    pub pattern: Expression,
}

/// `IFolderQuery`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IFolderQuery<U = URI> {
    pub folder: U,
    pub folder_name: Option<String>,
    pub exclude_pattern: Option<Vec<ExcludeGlobPattern<U>>>,
    pub include_pattern: Option<Expression>,
    pub ignore_glob_case: Option<bool>,
    pub file_encoding: Option<String>,
    pub disregard_ignore_files: Option<bool>,
    pub disregard_global_ignore_files: Option<bool>,
    pub disregard_parent_ignore_files: Option<bool>,
    pub ignore_symlinks: Option<bool>,
}

impl<U> IFolderQuery<U> {
    /// Um `{ folder }` sem nenhuma opção.
    pub fn new(folder: U) -> Self {
        Self {
            folder,
            folder_name: None,
            exclude_pattern: None,
            include_pattern: None,
            ignore_glob_case: None,
            file_encoding: None,
            disregard_ignore_files: None,
            disregard_global_ignore_files: None,
            disregard_parent_ignore_files: None,
            ignore_symlinks: None,
        }
    }
}

/// `ICommonQueryProps`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ICommonQueryProps<U = URI> {
    /// Para telemetria: o que dispara a busca (`_reason`).
    pub reason: Option<String>,
    pub folder_queries: Vec<IFolderQuery<U>>,
    /// O padrão de inclusão que vai para o ripgrep. Vale mais que os arquivos de ignore, se houver.
    pub include_pattern: Option<Expression>,
    pub exclude_pattern: Option<Expression>,
    pub ignore_glob_case: Option<bool>,
    pub extra_file_resources: Option<Vec<U>>,
    pub only_open_editors: Option<bool>,
    pub max_results: Option<i64>,
    pub using_search_paths: Option<bool>,
    pub only_file_scheme: Option<bool>,
}

impl<U> ICommonQueryProps<U> {
    /// Um `{ folderQueries }` sem nenhuma opção.
    pub fn new(folder_queries: Vec<IFolderQuery<U>>) -> Self {
        Self {
            reason: None,
            folder_queries,
            include_pattern: None,
            exclude_pattern: None,
            ignore_glob_case: None,
            extra_file_resources: None,
            only_open_editors: None,
            max_results: None,
            using_search_paths: None,
            only_file_scheme: None,
        }
    }
}

/// `ITextQueryProps` (`type: QueryType.Text`): `ITextQuery` é `ITextQueryProps<URI>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITextQueryProps<U = URI> {
    pub common: ICommonQueryProps<U>,
    pub content_pattern: PatternInfo,
    pub preview_options: Option<ITextSearchPreviewOptions>,
    pub max_file_size: Option<i64>,
    pub surrounding_context: Option<i64>,
    pub user_disabled_excludes_and_ignore_files: Option<bool>,
}

impl<U> ITextQueryProps<U> {
    /// Um `{ folderQueries, contentPattern }` sem nenhuma opção.
    pub fn new(folder_queries: Vec<IFolderQuery<U>>, content_pattern: PatternInfo) -> Self {
        Self {
            common: ICommonQueryProps::new(folder_queries),
            content_pattern,
            preview_options: None,
            max_file_size: None,
            surrounding_context: None,
            user_disabled_excludes_and_ignore_files: None,
        }
    }
}

/// `IAITextQueryProps` (`type: QueryType.aiText`): `IAITextQuery` é `IAITextQueryProps<URI>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IAITextQueryProps<U = URI> {
    pub common: ICommonQueryProps<U>,
    pub content_pattern: JsString,
    pub preview_options: Option<ITextSearchPreviewOptions>,
    pub max_file_size: Option<i64>,
    pub surrounding_context: Option<i64>,
    pub user_disabled_excludes_and_ignore_files: Option<bool>,
}

pub type ITextQuery = ITextQueryProps<URI>;
pub type IAITextQuery = IAITextQueryProps<URI>;

/// `IFileQueryProps` (`type: QueryType.File`): `IFileQuery` é `IFileQueryProps<URI>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IFileQueryProps<U = URI> {
    pub common: ICommonQueryProps<U>,
    pub file_pattern: Option<String>,
    /// When walking through the tree to find the result, don't use the filePattern to fuzzy match. Instead, should use
    /// glob matching.
    pub should_glob_match_file_pattern: Option<bool>,
    /// If true no results will be returned. Instead `limitHit` will indicate if at least one result exists or not.
    /// Currently does not work with queries including a 'siblings clause'.
    pub exists: Option<bool>,
    pub sort_by_score: Option<bool>,
    pub cache_key: Option<String>,
}

impl<U> IFileQueryProps<U> {
    /// Um `{ folderQueries }` sem nenhuma opção.
    pub fn new(folder_queries: Vec<IFolderQuery<U>>) -> Self {
        Self {
            common: ICommonQueryProps::new(folder_queries),
            file_pattern: None,
            should_glob_match_file_pattern: None,
            exists: None,
            sort_by_score: None,
            cache_key: None,
        }
    }
}

pub type IFileQuery = IFileQueryProps<URI>;

/// `QueryType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryType {
    File = 1,
    Text = 2,
    AiText = 3,
}

/// `ITextSearchQuery = ITextQuery | IAITextQuery`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ITextSearchQuery {
    Text(ITextQuery),
    AiText(IAITextQuery),
}

impl ITextSearchQuery {
    /// `query.surroundingContext`, comum às duas formas.
    pub fn surrounding_context(&self) -> Option<i64> {
        match self {
            ITextSearchQuery::Text(query) => query.surrounding_context,
            ITextSearchQuery::AiText(query) => query.surrounding_context,
        }
    }
}

/// `ITextSearchContext`: uma linha de contexto ao redor de um casamento.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITextSearchContext {
    pub text: JsString,
    pub line_number: i64,
}

/// `ITextSearchResult = ITextSearchMatch | ITextSearchContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ITextSearchResult {
    Match(TextSearchMatch),
    Context(ITextSearchContext),
}

impl ITextSearchResult {
    /// `resultIsMatch`.
    pub fn is_match(&self) -> bool {
        matches!(self, ITextSearchResult::Match(_))
    }
}

/// O `configuration && configuration.a && configuration.a.b` do original: o valor no caminho dado, ou `None` se algum
/// passo (ou o próprio valor) é falso no sentido do JavaScript.
fn truthy_property(configuration: &Dynamic, path: &[&str]) -> Option<Dynamic> {
    let mut current = configuration.clone();
    if !current.is_truthy() {
        return None;
    }
    for key in path {
        current = current.get_str(key);
        if !current.is_truthy() {
            return None;
        }
    }
    Some(current)
}

/// `getExcludes`: os `files.exclude` e, com `include_search_excludes`, os `search.exclude`, mesclados (os do `search`
/// vencem). `configuration` é o objeto `ISearchConfiguration` que o serviço de configuração devolve.
pub fn get_excludes(configuration: &Dynamic, include_search_excludes: bool) -> Option<Expression> {
    get_excludes_value(configuration, include_search_excludes).map(|excludes| Expression::from_dynamic(&excludes))
}

/// O valor de [`get_excludes`] antes de virar [`Expression`]: o objeto de padrões, para quem o entrega a um
/// `ResourceGlobMatcher` (que lê o objeto da configuração).
pub fn get_excludes_value(configuration: &Dynamic, include_search_excludes: bool) -> Option<Dynamic> {
    let file_excludes = truthy_property(configuration, &["files", "exclude"]);
    let search_excludes = if include_search_excludes { truthy_property(configuration, &["search", "exclude"]) } else { None };

    match (file_excludes, search_excludes) {
        (None, None) => None,
        (Some(excludes), None) | (None, Some(excludes)) => Some(excludes),
        (Some(file_excludes), Some(search_excludes)) => {
            // clone the config as it could be frozen
            let all_excludes = Dynamic::object(Vec::<(&str, Dynamic)>::new());
            let all_excludes = objects::mixin(&all_excludes, &objects::deep_clone(&file_excludes), true);
            let all_excludes = objects::mixin(&all_excludes, &objects::deep_clone(&search_excludes), true);
            Some(all_excludes)
        }
    }
}

/// `pathIncludedInQuery`.
pub fn path_included_in_query(query_props: &ICommonQueryProps<URI>, path_to_check: &str) -> bool {
    let ignore_glob_case = query_props.ignore_glob_case.is_truthy();
    let glob_options = if ignore_glob_case { GlobOptions { ignore_case: Some(true), ..GlobOptions::default() } } else { GlobOptions::default() };
    let expression_matches = |expression: &Expression, path: &str| {
        // O resultado de `glob.match` é o padrão que casou: verdadeiro se é um texto não vazio.
        glob::match_expression(expression, path, &glob_options).matched().is_some_and(|pattern| !pattern.is_empty())
    };

    if query_props.exclude_pattern.as_ref().is_some_and(|exclude| expression_matches(exclude, path_to_check)) {
        return false;
    }

    let using_search_paths = query_props.using_search_paths.is_truthy();
    if query_props.include_pattern.is_some() || using_search_paths {
        if query_props.include_pattern.as_ref().is_some_and(|include| expression_matches(include, path_to_check)) {
            return true;
        }

        // If searchPaths are being used, the extra file must be in a subfolder and match the pattern, if present
        if using_search_paths {
            let env = host_environment();
            return query_props.folder_queries.iter().any(|folder_query| {
                let search_path = fs_path(&folder_query.folder);
                if extpath::is_equal_or_parent(path_to_check, &search_path, ignore_glob_case, false, env.as_ref()) {
                    let rel_path = PathModule::native(env.as_ref()).relative(&search_path, path_to_check);
                    folder_query.include_pattern.as_ref().is_none_or(|include| expression_matches(include, &rel_path))
                } else {
                    false
                }
            });
        }

        return false;
    }

    true
}

// ---------------------------------------------------------------------------------------------------------
// Resultados da busca e o serviço (`IFileMatch`, `ISearchProgressItem`, `ISearchComplete`, `ISearchService`...)
//
// Divergências:
// - `IFileMatch<U>` é não genérico (`U = URI`), e as extensões estruturais do TypeScript `INotebookFileMatchNoModel` e
//   `INotebookFileMatchWithModel` (um `IFileMatch` com `cellResults`) são o campo `cell_results`: o `'cellResults' in
//   object` do original é `cell_results.is_some()`.
// - `ISearchConfigurationProperties` é o objeto [`Dynamic`] que o serviço de configuração devolve para a seção `search`
//   (como `get_excludes` faz com o `ISearchConfiguration`); só `searchOnType` é lido pelo modelo da árvore de resultados.
// - `ResourceSet` (que o original compartilha por referência) é o [`SharedResourceSet`].
// - Os callbacks `onProgress?: (result) => void` são `Option<SearchProgressCallback>`.
// ---------------------------------------------------------------------------------------------------------

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::DisposableRef;
use je_base::map::ResourceSet;
use je_base::promise::Promise;
use je_instantiation::service_id;

use super::search_ext_types::{AISearchKeyword, TextSearchCompleteMessageType};
use crate::contrib::search::notebook_search::search_notebook_helpers::INotebookCellMatch;

/// `DEFAULT_MAX_SEARCH_RESULTS`.
pub const DEFAULT_MAX_SEARCH_RESULTS: usize = 20000;

/// `SearchSortOrder` (os valores de texto do original são `default`, `fileNames`, `type`, `modified`, `countDescending` e
/// `countAscending`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchSortOrder {
    #[default]
    Default,
    FileNames,
    Type,
    Modified,
    CountDescending,
    CountAscending,
}

/// `IFileMatch`: um arquivo com os resultados de texto e, para notebooks, os resultados por célula.
#[derive(Clone)]
pub struct IFileMatch {
    pub resource: URI,
    pub results: Option<Vec<ITextSearchResult>>,
    pub cell_results: Option<Vec<INotebookCellMatch>>,
}

impl IFileMatch {
    /// Um `{ resource, results }` (sem `cellResults`).
    pub fn new(resource: URI, results: Option<Vec<ITextSearchResult>>) -> Self {
        Self { resource, results, cell_results: None }
    }
}

/// `IProgressMessage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IProgressMessage {
    pub message: String,
}

/// `ISearchProgressItem = IFileMatch | IProgressMessage | AISearchKeyword`.
#[derive(Clone)]
pub enum ISearchProgressItem {
    FileMatch(IFileMatch),
    Message(IProgressMessage),
    AiKeyword(AISearchKeyword),
}

impl ISearchProgressItem {
    /// `isFileMatch`.
    pub fn as_file_match(&self) -> Option<&IFileMatch> {
        match self {
            ISearchProgressItem::FileMatch(file_match) => Some(file_match),
            _ => None,
        }
    }

    /// `isAIKeyword`.
    pub fn as_ai_keyword(&self) -> Option<&AISearchKeyword> {
        match self {
            ISearchProgressItem::AiKeyword(keyword) => Some(keyword),
            _ => None,
        }
    }

    /// `isProgressMessage`: a mensagem existe e não é vazia.
    pub fn as_progress_message(&self) -> Option<&IProgressMessage> {
        match self {
            ISearchProgressItem::Message(message) if !message.message.is_empty() => Some(message),
            _ => None,
        }
    }
}

/// `ITextSearchCompleteMessage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITextSearchCompleteMessage {
    pub text: String,
    pub message_type: TextSearchCompleteMessageType,
    pub trusted: Option<bool>,
}

/// `SearchCompletionExitCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchCompletionExitCode {
    Normal = 0,
    NewSearchStarted = 1,
}

/// O `type` de `ITextSearchStats`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSearchStatsType {
    TextSearchProvider,
    SearchProcess,
    AiTextSearchProvider,
}

impl TextSearchStatsType {
    /// O texto do `type` no original.
    pub fn as_str(&self) -> &'static str {
        match self {
            TextSearchStatsType::TextSearchProvider => "textSearchProvider",
            TextSearchStatsType::SearchProcess => "searchProcess",
            TextSearchStatsType::AiTextSearchProvider => "aiTextSearchProvider",
        }
    }
}

/// `ITextSearchStats`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ITextSearchStats {
    pub stats_type: TextSearchStatsType,
}

/// `ICachedSearchStats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ICachedSearchStats {
    pub cache_was_resolved: bool,
    pub cache_lookup_time: f64,
    pub cache_filter_time: f64,
    pub cache_entry_count: f64,
}

/// `ISearchEngineStats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ISearchEngineStats {
    pub file_walk_time: f64,
    pub directories_walked: f64,
    pub files_walked: f64,
    pub cmd_time: f64,
    pub cmd_result_count: Option<f64>,
}

/// `IFileSearchProviderStats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IFileSearchProviderStats {
    pub provider_time: f64,
    pub post_process_time: f64,
}

/// `ISearchEngineStats | ICachedSearchStats | IFileSearchProviderStats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FileSearchDetailStats {
    Engine(ISearchEngineStats),
    Cached(ICachedSearchStats),
    Provider(IFileSearchProviderStats),
}

/// O `type` de `IFileSearchStats`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSearchStatsType {
    FileSearchProvider,
    SearchProcess,
}

/// `IFileSearchStats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IFileSearchStats {
    pub from_cache: bool,
    pub detail_stats: FileSearchDetailStats,
    pub result_count: f64,
    pub stats_type: FileSearchStatsType,
    pub sorting_time: Option<f64>,
}

/// `IFileSearchStats | ITextSearchStats`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ISearchStats {
    File(IFileSearchStats),
    Text(ITextSearchStats),
}

/// `ISearchComplete` (com os campos de `ISearchCompleteStats`).
#[derive(Clone)]
pub struct ISearchComplete {
    pub limit_hit: Option<bool>,
    pub messages: Vec<ITextSearchCompleteMessage>,
    pub stats: Option<ISearchStats>,
    pub results: Vec<IFileMatch>,
    pub exit: Option<SearchCompletionExitCode>,
    pub ai_keywords: Option<Vec<AISearchKeyword>>,
}

/// `ResourceSet` compartilhado: o original passa a mesma instância entre a busca de notebooks e a de texto.
pub type SharedResourceSet = Rc<RefCell<ResourceSet>>;

/// O `onProgress?: (result: ISearchProgressItem) => void` do original.
pub type SearchProgressCallback = Rc<dyn Fn(&ISearchProgressItem)>;

/// `SearchProviderType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchProviderType {
    File,
    Text,
    AiText,
}

/// `ISearchResultProvider`.
pub trait ISearchResultProvider {
    fn get_ai_name(&self) -> Promise<Option<String>>;
    fn text_search(&self, query: &ITextQuery, on_progress: Option<SearchProgressCallback>, token: Option<CancellationToken>) -> Promise<ISearchComplete>;
    fn file_search(&self, query: &IFileQuery, token: Option<CancellationToken>) -> Promise<ISearchComplete>;
    fn clear_cache(&self, cache_key: &str) -> Promise<()>;
}

/// O `{ syncResults, asyncResults }` de `textSearchSplitSyncAsync`.
pub struct TextSearchSplitSyncAsync {
    pub sync_results: ISearchComplete,
    pub async_results: Promise<ISearchComplete>,
}

/// `ISearchService`: um serviço que permite buscar arquivos ou dentro de arquivos.
pub trait ISearchService {
    fn text_search(&self, query: &ITextQuery, token: Option<CancellationToken>, on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete>;
    fn ai_text_search(&self, query: &IAITextQuery, token: Option<CancellationToken>, on_progress: Option<SearchProgressCallback>) -> Promise<ISearchComplete>;
    fn get_ai_name(&self) -> Promise<Option<String>>;
    fn text_search_split_sync_async(
        &self,
        query: &ITextQuery,
        token: Option<CancellationToken>,
        on_progress: Option<SearchProgressCallback>,
        notebook_files_to_ignore: Option<SharedResourceSet>,
        async_notebook_files_to_ignore: Option<Promise<SharedResourceSet>>,
    ) -> TextSearchSplitSyncAsync;
    fn file_search(&self, query: &IFileQuery, token: Option<CancellationToken>) -> Promise<ISearchComplete>;
    fn scheme_has_file_search_provider(&self, scheme: &str) -> bool;
    fn clear_cache(&self, cache_key: &str) -> Promise<()>;
    fn register_search_result_provider(&self, scheme: &str, provider_type: SearchProviderType, provider: Rc<dyn ISearchResultProvider>) -> DisposableRef;
}

service_id!(pub I_SEARCH_SERVICE: dyn ISearchService = "searchService");
