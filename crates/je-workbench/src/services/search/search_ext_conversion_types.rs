//! Porte de `src/vs/workbench/services/search/common/searchExtConversionTypes.ts`: os tipos antigos da API de busca das
//! extensões (`FileSearchProvider`, `TextSearchProvider` e as opções, consultas e resultados deles), mantidos por
//! compatibilidade, e os conversores `OldFileSearchProviderConverter` e `OldTextSearchProviderConverter`, que fazem um
//! provedor antigo valer como `FileSearchProvider2` e `TextSearchProvider2`.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `interface TextSearchOptions extends SearchOptions` (e `FileSearchOptions`, `AITextSearchOptions`) vira a composição:
//!   o campo `common` com as [`SearchOptions`]. O `previewOptions` é o [`ITextSearchPreviewOptions`] (mesma forma que
//!   `TextSearchPreviewOptions`).
//! - `TextSearchQuery` tem a mesma forma que `TextSearchQuery2` (o conversor repassa a consulta nova como antiga), então é um
//!   alias.
//! - `Range | Range[]` (`ranges` e `preview.matches`) é [`RangeOrRanges`], que guarda se o valor era uma lista (o que
//!   `validateProviderResult` confere); `asArray` é [`RangeOrRanges::as_array`].
//! - `TextSearchMatch | TextSearchContext` é o enum [`TextSearchResult`]; o `isTextSearchMatch` do original, que testa as
//!   propriedades, é a variante.
//! - `ProviderResult<T>` é a promessa de um `Option` ([`ProviderResult`]), e `null`/`undefined` são `None`. Uma exceção
//!   lançada pelo provedor antigo dentro do `async getResult` do original rejeita a promessa; aqui o `errors::catch` faz o
//!   mesmo, e a primeira exceção interrompe as chamadas seguintes (o `map` do original aborta).
//! - `session?: CancellationToken` de `FileSearchOptions` sai do `session: unknown` do `FileSearchProviderOptions`: o
//!   `FileSearchManager` entrega o token da sessão a um provedor antigo ([`FileSearchProvider2::is_old_file_search_provider_converter`]),
//!   e um objeto de sessão comum (de um provedor novo, ou a ausência de sessão) é `None`.
//! - `newToOldPreviewOptions` recebe o [`ITextSearchPreviewOptions`] já completo (os campos opcionais do original nunca
//!   faltam nos tipos de Rust) e devolve as opções padrão quando ele falta.
//! - o `console.warn` de `validateProviderResult` é `eprintln!`.
//! - `FindTextInFilesOptions` guarda `useParentIgnoreFiles` como `bool` obrigatório, como no original.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::js_string::JsString;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::progress::progress::IProgress;

use super::search::{DEFAULT_TEXT_SEARCH_PREVIEW_OPTIONS, ITextSearchPreviewOptions};
use super::search_ext_types::{
    FileSearchProvider2, FileSearchProviderOptions, GlobPattern as NewGlobPattern, ProviderResult, Range, TextSearchComplete2, TextSearchCompleteMessage2, TextSearchContext2,
    TextSearchMatch2, TextSearchMatchRanges, TextSearchProvider2, TextSearchProviderOptions, TextSearchQuery2, TextSearchResult2,
};

// old types that are retained for backward compatibility
// TODO: delete this when search apis are adopted by all first-party extensions

/// `RelativePattern`: um glob casado relativamente a um caminho base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelativePattern {
    /// A base file path to which this pattern will be matched against relatively.
    pub base: String,
    /// A file glob pattern like `*.{ts,js}` that will be matched on file paths relative to the base path.
    pub pattern: String,
}

/// `GlobPattern = string | RelativePattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobPattern {
    Pattern(String),
    Relative(RelativePattern),
}

/// `TextSearchQuery`: os parâmetros de uma consulta de busca de texto (a mesma forma que [`TextSearchQuery2`]).
pub type TextSearchQuery = TextSearchQuery2;

/// `GlobString`.
pub type GlobString = String;

/// `SearchOptions`: opções comuns às buscas de arquivo e de texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    /// The root folder to search within.
    pub folder: URI,
    /// Files that match an `includes` glob pattern should be included in the search.
    pub includes: Vec<GlobString>,
    /// Files that match an `excludes` glob pattern should be excluded from the search.
    pub excludes: Vec<GlobString>,
    /// Whether external files that exclude files, like .gitignore, should be respected (`search.useIgnoreFiles`).
    pub use_ignore_files: bool,
    /// Whether symlinks should be followed while searching (`search.followSymlinks`).
    pub follow_symlinks: bool,
    /// Whether global files that exclude files, like .gitignore, should be respected (`search.useGlobalIgnoreFiles`).
    pub use_global_ignore_files: bool,
    /// Whether files in parent directories that exclude files, like .gitignore, should be respected
    /// (`search.useParentIgnoreFiles`).
    pub use_parent_ignore_files: bool,
}

/// `TextSearchOptions`: opções da busca de texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchOptions {
    pub common: SearchOptions,
    /// The maximum number of results to be returned.
    pub max_results: i64,
    /// Options to specify the size of the result text preview.
    pub preview_options: Option<ITextSearchPreviewOptions>,
    /// Exclude files larger than `maxFileSize` in bytes.
    pub max_file_size: Option<i64>,
    /// Interpret files using this encoding (`files.encoding`).
    pub encoding: Option<String>,
    /// Number of lines of context to include before each match.
    pub before_context: Option<i64>,
    /// Number of lines of context to include after each match.
    pub after_context: Option<i64>,
}

/// `AITextSearchOptions`: opções da busca de texto com IA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AITextSearchOptions {
    pub common: SearchOptions,
    /// The maximum number of results to be returned.
    pub max_results: i64,
    /// Options to specify the size of the result text preview.
    pub preview_options: Option<ITextSearchPreviewOptions>,
    /// Exclude files larger than `maxFileSize` in bytes.
    pub max_file_size: Option<i64>,
    /// Number of lines of context to include before each match.
    pub before_context: Option<i64>,
    /// Number of lines of context to include after each match.
    pub after_context: Option<i64>,
}

/// `TextSearchComplete`: o que se sabe quando a busca de texto termina. O `message` é sempre uma lista (um valor só é uma
/// lista de um elemento).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSearchComplete {
    /// Whether the search hit the limit on the maximum number of search results.
    pub limit_hit: Option<bool>,
    /// Additional information regarding the state of the completed search.
    pub message: Option<Vec<TextSearchCompleteMessage2>>,
}

/// `FileSearchQuery`: os parâmetros de uma consulta de busca de arquivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSearchQuery {
    /// The search pattern to match against file paths.
    pub pattern: String,
}

/// `FileSearchOptions`: opções da busca de arquivo.
#[derive(Clone)]
pub struct FileSearchOptions {
    pub common: SearchOptions,
    /// The maximum number of results to be returned.
    pub max_results: Option<i64>,
    /// A CancellationToken that represents the session for this search query. If the provider chooses to, this object can be
    /// used as the key for a cache, and searches with the same session object can search the same cache. When the token is
    /// cancelled, the session is complete and the cache can be cleared.
    pub session: Option<CancellationToken>,
}

/// `Range | Range[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RangeOrRanges {
    One(Range),
    Many(Vec<Range>),
}

impl RangeOrRanges {
    /// `Array.isArray(value)`.
    pub fn is_array(&self) -> bool {
        matches!(self, RangeOrRanges::Many(_))
    }

    /// `asArray(value)`.
    pub fn as_array(&self) -> Vec<Range> {
        match self {
            RangeOrRanges::One(range) => vec![*range],
            RangeOrRanges::Many(ranges) => ranges.clone(),
        }
    }
}

/// `TextSearchMatchPreview`: a prévia do texto do resultado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchMatchPreview {
    /// The matching lines of text, or a portion of the matching line that contains the match.
    pub text: JsString,
    /// The Range within `text` corresponding to the text of the match. The number of matches must match the
    /// TextSearchMatch's range property.
    pub matches: RangeOrRanges,
}

/// `TextSearchMatch`: um casamento de uma busca de texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchMatch {
    /// The uri for the matching document.
    pub uri: URI,
    /// The range of the match within the document, or multiple ranges for multiple matches.
    pub ranges: RangeOrRanges,
    /// A preview of the text match.
    pub preview: TextSearchMatchPreview,
}

/// `TextSearchContext`: uma linha de contexto ao redor de um [`TextSearchMatch`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchContext {
    /// The uri for the matching document.
    pub uri: URI,
    /// One line of text. previewOptions.charsPerLine applies to this.
    pub text: JsString,
    /// The line number of this line of context.
    pub line_number: i64,
}

/// `TextSearchResult = TextSearchMatch | TextSearchContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextSearchResult {
    Match(TextSearchMatch),
    Context(TextSearchContext),
}

/// `FileSearchProvider`: provides search results for files in the given folder that match a query string. It can be invoked by
/// quickaccess or other extensions.
pub trait FileSearchProvider {
    /// Provide the set of files that match a certain file path pattern.
    fn provide_file_search_results(&self, query: &FileSearchQuery, options: &FileSearchOptions, token: &CancellationToken) -> ProviderResult<Vec<URI>>;
}

/// `TextSearchProvider`: provides search results for text results inside files in the workspace.
pub trait TextSearchProvider {
    /// Provide results that match the given text pattern.
    fn provide_text_search_results(
        &self,
        query: &TextSearchQuery,
        options: &TextSearchOptions,
        progress: Rc<dyn IProgress<TextSearchResult>>,
        token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete>;
}

/// `FindTextInFilesOptions`: opções de um `findTextInFiles`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindTextInFilesOptions {
    /// A glob pattern that defines the files to search for.
    pub include: Option<GlobPattern>,
    /// A glob pattern that defines files and folders to exclude. `None` is `undefined` (only default excludes apply); the
    /// `null` of the original (no excludes at all) is `Some(None)`.
    pub exclude: Option<Option<GlobPattern>>,
    /// The maximum number of results to search for.
    pub max_results: Option<i64>,
    /// Whether external files that exclude files, like .gitignore, should be respected.
    pub use_ignore_files: Option<bool>,
    /// Whether global files that exclude files, like .gitignore, should be respected.
    pub use_global_ignore_files: Option<bool>,
    /// Whether files in parent directories that exclude files, like .gitignore, should be respected.
    pub use_parent_ignore_files: bool,
    /// Whether symlinks should be followed while searching.
    pub follow_symlinks: Option<bool>,
    /// Interpret files using this encoding.
    pub encoding: Option<String>,
    /// Options to specify the size of the result text preview.
    pub preview_options: Option<ITextSearchPreviewOptions>,
    /// Number of lines of context to include before each match.
    pub before_context: Option<i64>,
    /// Number of lines of context to include after each match.
    pub after_context: Option<i64>,
}

/// O texto de um glob da API nova: o de `string`, ou o `pattern` de um `RelativePattern`.
fn glob_pattern_text(pattern: &NewGlobPattern) -> String {
    match pattern {
        NewGlobPattern::Pattern(pattern) => pattern.clone(),
        NewGlobPattern::Relative(relative) => relative.pattern.clone(),
    }
}

/// `newToOldFileProviderOptions`.
fn new_to_old_file_provider_options(options: &FileSearchProviderOptions) -> Vec<FileSearchOptions> {
    options
        .folder_options
        .iter()
        .map(|folder_option| FileSearchOptions {
            common: SearchOptions {
                folder: folder_option.folder.clone(),
                excludes: folder_option.excludes.iter().map(glob_pattern_text).collect(),
                includes: folder_option.includes.clone(),
                use_global_ignore_files: folder_option.use_ignore_files.global,
                use_ignore_files: folder_option.use_ignore_files.local,
                use_parent_ignore_files: folder_option.use_ignore_files.parent,
                follow_symlinks: folder_option.follow_symlinks,
            },
            max_results: Some(options.max_results),
            // TODO do original: make sure that we actually use a cancellation token here.
            session: options.session.downcast_ref::<CancellationToken>().cloned(),
        })
        .collect()
}

/// `OldFileSearchProviderConverter`.
pub struct OldFileSearchProviderConverter {
    provider: Rc<dyn FileSearchProvider>,
}

impl OldFileSearchProviderConverter {
    pub fn new(provider: Rc<dyn FileSearchProvider>) -> Self {
        Self { provider }
    }
}

impl FileSearchProvider2 for OldFileSearchProviderConverter {
    fn provide_file_search_results(&self, pattern: &str, options: &FileSearchProviderOptions, token: &CancellationToken) -> ProviderResult<Vec<URI>> {
        let query = FileSearchQuery { pattern: pattern.to_owned() };
        let mut results = Vec::new();
        for old_options in new_to_old_file_provider_options(options) {
            match errors::catch(|| self.provider.provide_file_search_results(&query, &old_options, token)) {
                Ok(result) => results.push(result),
                Err(error) => return Promise::rejected(error),
            }
        }
        Promise::all(results).then(|results| Some(results.into_iter().flatten().flatten().collect()))
    }

    fn is_old_file_search_provider_converter(&self) -> bool {
        true
    }
}

/// `newToOldTextProviderOptions`.
fn new_to_old_text_provider_options(options: &TextSearchProviderOptions) -> Vec<TextSearchOptions> {
    options
        .folder_options
        .iter()
        .map(|folder_option| TextSearchOptions {
            common: SearchOptions {
                folder: folder_option.folder.clone(),
                excludes: folder_option.excludes.iter().map(glob_pattern_text).collect(),
                includes: folder_option.includes.clone(),
                use_global_ignore_files: folder_option.use_ignore_files.global,
                use_ignore_files: folder_option.use_ignore_files.local,
                use_parent_ignore_files: folder_option.use_ignore_files.parent,
                follow_symlinks: folder_option.follow_symlinks,
            },
            max_results: options.max_results,
            preview_options: Some(new_to_old_preview_options(Some(options.preview_options))),
            max_file_size: options.max_file_size,
            encoding: Some(folder_option.encoding.clone()),
            after_context: Some(options.surrounding_context),
            before_context: Some(options.surrounding_context),
        })
        .collect()
}

/// `newToOldPreviewOptions`.
pub fn new_to_old_preview_options(options: Option<ITextSearchPreviewOptions>) -> ITextSearchPreviewOptions {
    options.unwrap_or(DEFAULT_TEXT_SEARCH_PREVIEW_OPTIONS)
}

/// `oldToNewTextSearchResult`.
pub fn old_to_new_text_search_result(result: &TextSearchResult) -> TextSearchResult2 {
    match result {
        TextSearchResult::Match(text_match) => {
            let preview_arr = text_match.preview.matches.as_array();
            let ranges = text_match
                .ranges
                .as_array()
                .into_iter()
                .enumerate()
                .map(|(i, range)| {
                    let matching_preview_range = preview_arr
                        .get(i)
                        .copied()
                        .unwrap_or_else(|| errors::throw(Error::type_error("Cannot read properties of undefined (reading 'start')")));
                    TextSearchMatchRanges { source_range: range, preview_range: matching_preview_range }
                })
                .collect();
            TextSearchResult2::Match(TextSearchMatch2::new(text_match.uri.clone(), ranges, text_match.preview.text.clone()))
        }
        TextSearchResult::Context(context) => TextSearchResult2::Context(TextSearchContext2::new(context.uri.clone(), context.text.clone(), context.line_number)),
    }
}

/// O `{ report: (e) => progressShim(e) }` que o conversor entrega ao provedor antigo.
struct ProgressShim {
    progress: Rc<dyn IProgress<TextSearchResult2>>,
}

impl IProgress<TextSearchResult> for ProgressShim {
    fn report(&self, old_result: TextSearchResult) {
        if !validate_provider_result(&old_result) {
            return;
        }
        self.progress.report(old_to_new_text_search_result(&old_result));
    }
}

/// `OldTextSearchProviderConverter`.
pub struct OldTextSearchProviderConverter {
    provider: Rc<dyn TextSearchProvider>,
}

impl OldTextSearchProviderConverter {
    pub fn new(provider: Rc<dyn TextSearchProvider>) -> Self {
        Self { provider }
    }
}

impl TextSearchProvider2 for OldTextSearchProviderConverter {
    fn provide_text_search_results(
        &self,
        query: &TextSearchQuery2,
        options: &TextSearchProviderOptions,
        progress: Rc<dyn IProgress<TextSearchResult2>>,
        token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete2> {
        let progress_shim: Rc<dyn IProgress<TextSearchResult>> = Rc::new(ProgressShim { progress });

        let mut results = Vec::new();
        for old_options in new_to_old_text_provider_options(options) {
            match errors::catch(|| self.provider.provide_text_search_results(query, &old_options, progress_shim.clone(), token)) {
                Ok(result) => results.push(result),
                Err(error) => return Promise::rejected(error),
            }
        }

        Promise::all(results).then(|results| {
            // `reduce((prev, cur) => ({ limitHit: prev.limitHit || cur.limitHit }), { limitHit: false })`
            let limit_hit = results
                .into_iter()
                .flatten()
                .fold(Some(false), |prev, cur| if prev.unwrap_or(false) { prev } else { cur.limit_hit });
            // O `reduce` descarta o `message` de cada resultado: `coalesce(asArray(undefined))` é a lista vazia.
            Some(TextSearchComplete2 { limit_hit, message: Some(Vec::new()) })
        })
    }
}

/// `validateProviderResult`.
fn validate_provider_result(result: &TextSearchResult) -> bool {
    if let TextSearchResult::Match(text_match) = result {
        match (&text_match.ranges, &text_match.preview.matches) {
            (RangeOrRanges::Many(ranges), RangeOrRanges::Many(matches)) => {
                if matches.len() != ranges.len() {
                    eprintln!("INVALID - A text search provider match's`ranges` and`matches` properties must have the same length.");
                    return false;
                }
            }
            (RangeOrRanges::Many(_), RangeOrRanges::One(_)) => {
                eprintln!("INVALID - A text search provider match's`ranges` and`matches` properties must have the same type.");
                return false;
            }
            (RangeOrRanges::One(_), RangeOrRanges::Many(_)) => {
                eprintln!("INVALID - A text search provider match's`ranges` and`matches` properties must have the same length.");
                return false;
            }
            (RangeOrRanges::One(_), RangeOrRanges::One(_)) => {}
        }
    }

    true
}

/// `extensionResultIsMatch`.
pub fn extension_result_is_match(data: &TextSearchResult) -> bool {
    matches!(data, TextSearchResult::Match(_))
}
