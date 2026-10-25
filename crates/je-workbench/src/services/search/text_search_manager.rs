//! Porte de `src/vs/workbench/services/search/common/textSearchManager.ts` (`TextSearchManager`,
//! `TextSearchResultsCollector`, `BatchedCollector`) e de `node/textSearchManager.ts` (`NativeTextSearchManager`, que
//! só liga o gerenciador comum ao disco), no mesmo módulo porque os dois arquivos têm o mesmo nome.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `Promise` é a [`Promise`] do porte. `search` devolve a promessa do `ISearchCompleteStats`; um erro do provedor
//!   rejeita com `new Error(toErrorMessage(err))`, como no original.
//! - `IAITextQueryProviderPair | ITextQueryProviderPair` é o enum [`TextQueryProviderPair`], e `this.query` é o
//!   [`ICommonQueryProps`] das duas formas (as consultas compõem o `common`).
//! - `onProgress` e `onKeywordResult` são fechos `Rc<dyn Fn>`; um `BatchedCollector` recebe o fecho `cb` do mesmo jeito.
//! - `IFileUtils` é a struct de fechos [`IFileUtils`]. `fileUtils.readdir` devolve a promessa da lista de nomes.
//! - `progress.report(result)` aceita `TextSearchResult2 | AISearchResult`: é o fecho de [`TextSearchManagerInner::do_search`],
//!   entregue ao provedor de texto como `IProgress<TextSearchResult2>` e ao de IA como `IProgress<AISearchResult>`.
//! - `result.uri === undefined` (erro de provedor mal escrito) não existe: o `uri` de um resultado é obrigatório no tipo.
//! - `isThenable(included)` é sempre verdadeiro: `QueryGlobTester.includedInQuery` devolve sempre uma promessa, então o
//!   ramo `else if (included)` do original nunca roda e não é portado.
//! - `getSearchOptionsForFolder` passa `{ folder, patterns }` para `excludeToGlobPattern`, que só lê `baseUri`: aqui o
//!   `base_uri` é sempre `None`, o mesmo efeito.
//! - o `Timeout` do `BatchedCollector` é o [`TimerId`] do `setTimeout` do porte.
//! - `NativeTextSearchManager extends TextSearchManager` é a fábrica [`NativeTextSearchManager::new`]; o `_pfs: typeof
//!   pfs = pfs` é o fecho `readdir` de [`NativeTextSearchManager::with_readdir`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::error_message::to_error_message_of;
use je_base::errors::{self, Error};
use je_base::glob::Sibling;
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_base::uri::URI;
use je_platform::files::pfs;
use je_platform::progress::progress::IProgress;
use je_platform::resources::{ext_uri, host_environment};

use super::folder_query_search_tree::FolderQuerySearchTree;
use super::search::{
    DEFAULT_MAX_SEARCH_RESULTS, DEFAULT_TEXT_SEARCH_PREVIEW_OPTIONS, ExcludesForFolder, IAITextQuery, ICommonQueryProps, IFileMatch, IFolderQuery, ISearchCompleteStats, ISearchStats,
    ITextQuery, ITextSearchCompleteMessage, ITextSearchContext, ITextSearchPreviewOptions, ITextSearchResult, ITextSearchStats, PatternInfo, QueryGlobTester, SearchRange,
    SearchRangeSetPairing, SiblingFn, TextSearchMatch, TextSearchStatsType, exclude_to_glob_pattern, has_sibling_promise_fn, resolve_patterns_for_provider,
};
use super::search_ext_types::{
    AISearchKeyword, AISearchResult, AITextSearchProvider, ProviderResult, TextSearchComplete2, TextSearchMatch2, TextSearchProvider2, TextSearchProviderFolderOptions,
    TextSearchProviderOptions, TextSearchQuery2, TextSearchResult2, UseIgnoreFiles,
};
use crate::services::textfile::encoding::to_canonical_name;

/// `IFileUtils`.
#[derive(Clone)]
pub struct IFileUtils {
    pub readdir: Rc<dyn Fn(&URI) -> Promise<Vec<String>>>,
    pub to_canonical_name: Rc<dyn Fn(&str) -> String>,
}

/// `IAITextQueryProviderPair | ITextQueryProviderPair`.
#[derive(Clone)]
pub enum TextQueryProviderPair {
    Ai { query: IAITextQuery, provider: Rc<dyn AITextSearchProvider> },
    Text { query: ITextQuery, provider: Rc<dyn TextSearchProvider2> },
}

impl TextQueryProviderPair {
    /// `this.query`, nas partes que as duas formas de consulta têm em comum.
    fn common(&self) -> &ICommonQueryProps<URI> {
        match self {
            TextQueryProviderPair::Ai { query, .. } => &query.common,
            TextQueryProviderPair::Text { query, .. } => &query.common,
        }
    }

    fn preview_options(&self) -> Option<ITextSearchPreviewOptions> {
        match self {
            TextQueryProviderPair::Ai { query, .. } => query.preview_options,
            TextQueryProviderPair::Text { query, .. } => query.preview_options,
        }
    }

    fn max_file_size(&self) -> Option<i64> {
        match self {
            TextQueryProviderPair::Ai { query, .. } => query.max_file_size,
            TextQueryProviderPair::Text { query, .. } => query.max_file_size,
        }
    }

    fn surrounding_context(&self) -> Option<i64> {
        match self {
            TextQueryProviderPair::Ai { query, .. } => query.surrounding_context,
            TextQueryProviderPair::Text { query, .. } => query.surrounding_context,
        }
    }
}

/// `FolderQueryInfo`.
#[derive(Clone)]
struct FolderQueryInfo {
    query_tester: Rc<QueryGlobTester>,
    folder: URI,
    folder_idx: usize,
}

/// O progresso que o provedor de texto recebe (`IProgress<TextSearchResult2>`).
struct TextProgress(Rc<dyn Fn(AISearchResult)>);

impl IProgress<TextSearchResult2> for TextProgress {
    fn report(&self, item: TextSearchResult2) {
        (self.0)(AISearchResult::Text(item));
    }
}

/// O progresso que o provedor de IA recebe (`IProgress<AISearchResult>`).
struct AiProgress(Rc<dyn Fn(AISearchResult)>);

impl IProgress<AISearchResult> for AiProgress {
    fn report(&self, item: AISearchResult) {
        (self.0)(item);
    }
}

/// O `result.uri` das duas formas de resultado.
fn text_search_result_uri(result: &TextSearchResult2) -> &URI {
    match result {
        TextSearchResult2::Match(text_match) => &text_match.uri,
        TextSearchResult2::Context(context) => &context.uri,
    }
}

/// O estado de um [`TextSearchManager`], compartilhado com os fechos de uma busca em andamento.
struct TextSearchManagerInner {
    collector: RefCell<Option<Rc<RefCell<TextSearchResultsCollector>>>>,
    is_limit_hit: Cell<bool>,
    result_count: Cell<i64>,
    query_provider_pair: TextQueryProviderPair,
    file_utils: IFileUtils,
    process_type: TextSearchStatsType,
}

/// `TextSearchManager`.
pub struct TextSearchManager {
    inner: Rc<TextSearchManagerInner>,
}

impl TextSearchManager {
    pub fn new(query_provider_pair: TextQueryProviderPair, file_utils: IFileUtils, process_type: TextSearchStatsType) -> Self {
        Self {
            inner: Rc::new(TextSearchManagerInner {
                collector: RefCell::new(None),
                is_limit_hit: Cell::new(false),
                result_count: Cell::new(0),
                query_provider_pair,
                file_utils,
                process_type,
            }),
        }
    }

    pub fn search(&self, on_progress: Rc<dyn Fn(Vec<IFileMatch>)>, token: &CancellationToken, on_keyword_result: Option<Rc<dyn Fn(&AISearchKeyword)>>) -> Promise<ISearchCompleteStats> {
        let inner = self.inner.clone();
        let folder_queries = inner.query_provider_pair.common().folder_queries.clone();
        let token_source = Rc::new(CancellationTokenSource::new(Some(token)));

        let collector = Rc::new(RefCell::new(TextSearchResultsCollector::new(on_progress)));
        *inner.collector.borrow_mut() = Some(collector.clone());

        let is_canceled = Rc::new(Cell::new(false));
        let on_result: Rc<dyn Fn(TextSearchResult2, usize)> = {
            let inner = inner.clone();
            let collector = collector.clone();
            let token_source = token_source.clone();
            Rc::new(move |result: TextSearchResult2, folder_idx: usize| {
                if is_canceled.get() {
                    return;
                }

                if !inner.is_limit_hit.get() {
                    let mut result = result;
                    let result_count = inner.result_count.get();
                    let result_size = TextSearchManagerInner::result_size(&result);
                    let max_results = inner.query_provider_pair.common().max_results;
                    let trimmed = match (&result, max_results) {
                        (TextSearchResult2::Match(text_match), Some(max)) if result_count + result_size > max => {
                            Some(TextSearchManagerInner::trim_result_to_size(text_match, max - result_count))
                        }
                        _ => None,
                    };
                    if let Some(trimmed) = trimmed {
                        inner.is_limit_hit.set(true);
                        is_canceled.set(true);
                        token_source.cancel();

                        result = TextSearchResult2::Match(trimmed);
                    }

                    let new_result_size = TextSearchManagerInner::result_size(&result);
                    inner.result_count.set(inner.result_count.get() + new_result_size);
                    let a = matches!(result, TextSearchResult2::Match(_));

                    if new_result_size > 0 || !a {
                        collector.borrow_mut().add(result, folder_idx);
                    }
                }
            })
        };

        // For each root folder
        let finished = inner.clone();
        inner.do_search(folder_queries, on_result, token_source.token(), on_keyword_result).then_result(move |result| match result {
            Ok(result) => {
                token_source.dispose();
                collector.borrow_mut().flush();

                let limit_hit = if finished.is_limit_hit.get() { Some(true) } else { result.as_ref().and_then(|complete| complete.limit_hit) };
                Ok(ISearchCompleteStats {
                    limit_hit,
                    messages: TextSearchManagerInner::messages_from_results(result.as_ref()),
                    stats: Some(ISearchStats::Text(ITextSearchStats { stats_type: finished.process_type })),
                })
            }
            Err(err) => {
                token_source.dispose();
                Err(Error::new(to_error_message_of(&err, false)))
            }
        })
    }
}

impl TextSearchManagerInner {
    fn messages_from_results(result: Option<&TextSearchComplete2>) -> Vec<ITextSearchCompleteMessage> {
        let Some(messages) = result.and_then(|complete| complete.message.as_ref()) else {
            return Vec::new();
        };
        messages
            .iter()
            .map(|message| ITextSearchCompleteMessage { text: message.text.clone(), message_type: message.message_type, trusted: message.trusted })
            .collect()
    }

    fn result_size(result: &TextSearchResult2) -> i64 {
        match result {
            TextSearchResult2::Match(text_match) => text_match.ranges.len() as i64,
            // #104400 context lines shoudn't count towards result count
            TextSearchResult2::Context(_) => 0,
        }
    }

    fn trim_result_to_size(result: &TextSearchMatch2, size: i64) -> TextSearchMatch2 {
        let size = usize::try_from(size).unwrap_or(0).min(result.ranges.len());
        TextSearchMatch2::new(result.uri.clone(), result.ranges[..size].to_vec(), result.preview_text.clone())
    }

    fn do_search(
        self: &Rc<Self>,
        folder_queries: Vec<IFolderQuery<URI>>,
        on_result: Rc<dyn Fn(TextSearchResult2, usize)>,
        token: CancellationToken,
        on_keyword_result: Option<Rc<dyn Fn(&AISearchKeyword)>>,
    ) -> Promise<Option<TextSearchComplete2>> {
        let common = self.query_provider_pair.common();
        let folder_mappings: Rc<FolderQuerySearchTree<FolderQueryInfo>> = Rc::new(FolderQuerySearchTree::new(
            &folder_queries,
            |fq, i| FolderQueryInfo { query_tester: Rc::new(QueryGlobTester::new(common, fq)), folder: fq.folder.clone(), folder_idx: i },
            Some(Rc::new(|_: &URI| true)),
        ));

        let testing_ps: Rc<RefCell<Vec<Promise<()>>>> = Rc::new(RefCell::new(Vec::new()));
        let report: Rc<dyn Fn(AISearchResult)> = {
            let inner = self.clone();
            let testing_ps = testing_ps.clone();
            Rc::new(move |result: AISearchResult| {
                let result = match result {
                    AISearchResult::Keyword(keyword) => {
                        if let Some(on_keyword_result) = &on_keyword_result {
                            on_keyword_result(&keyword);
                        }
                        return;
                    }
                    AISearchResult::Text(result) => result,
                };
                let uri = text_search_result_uri(&result).clone();
                let Some(folder_query) = folder_mappings.find_query_fragment_aware_substr(&uri).cloned() else {
                    return;
                };
                if folder_query.folder.scheme().is_empty() {
                    return;
                }
                let has_sibling: Option<SiblingFn> = if folder_query.folder.scheme() == schemas::FILE {
                    let readdir = inner.file_utils.readdir.clone();
                    let directory = ext_uri().dirname(&uri);
                    let siblings_fn: Rc<dyn Fn() -> Promise<Vec<String>>> = Rc::new(move || readdir(&directory));
                    has_sibling_promise_fn(Some(siblings_fn)).map(|has_sibling| Rc::new(move |name: &str| Sibling::Pending(has_sibling(name))) as SiblingFn)
                } else {
                    None
                };

                let relative_path = ext_uri().relative_path(&folder_query.folder, &uri).filter(|relative_path| !relative_path.is_empty());
                if let Some(relative_path) = relative_path {
                    // This method is only async when the exclude contains sibling clauses
                    let env = host_environment();
                    let basename = PathModule::native(env.as_ref()).basename(&relative_path, None);
                    let included = folder_query.query_tester.included_in_query(&relative_path, Some(&basename), has_sibling);
                    let on_result = on_result.clone();
                    let folder_idx = folder_query.folder_idx;
                    testing_ps.borrow_mut().push(included.then(move |is_included| {
                        if is_included {
                            on_result(result, folder_idx);
                        }
                    }));
                }
            })
        };

        let folder_options = folder_queries.iter().map(|fq| self.get_search_options_for_folder(fq)).collect();
        let search_options = TextSearchProviderOptions {
            folder_options,
            max_file_size: self.query_provider_pair.max_file_size(),
            max_results: common.max_results.unwrap_or(DEFAULT_MAX_SEARCH_RESULTS as i64),
            preview_options: self.query_provider_pair.preview_options().unwrap_or(DEFAULT_TEXT_SEARCH_PREVIEW_OPTIONS),
            surrounding_context: self.query_provider_pair.surrounding_context().unwrap_or(0),
        };

        let provided = errors::catch(|| match &self.query_provider_pair {
            TextQueryProviderPair::Ai { query, provider } => {
                provider.provide_ai_text_search_results(&query.content_pattern, &search_options, Rc::new(AiProgress(report)), &token)
            }
            TextQueryProviderPair::Text { query, provider } => {
                provider.provide_text_search_results(&pattern_info_to_query(&query.content_pattern), &search_options, Rc::new(TextProgress(report)), &token)
            }
        });
        let provided: ProviderResult<TextSearchComplete2> = match provided {
            Ok(provided) => provided,
            Err(error) => return Promise::rejected(error),
        };

        provided.then_promise(move |result| {
            if testing_ps.borrow().is_empty() {
                return Promise::resolved(result);
            }
            let pending = testing_ps.borrow().clone();
            Promise::all(pending).then(move |_| result)
        })
    }

    fn get_search_options_for_folder(&self, fq: &IFolderQuery<URI>) -> TextSearchProviderFolderOptions {
        let common = self.query_provider_pair.common();
        let includes = resolve_patterns_for_provider(common.include_pattern.as_ref(), fq.include_pattern.as_ref());

        let mut exclude_pattern: Vec<ExcludesForFolder> = fq
            .exclude_pattern
            .as_ref()
            .map(|patterns| {
                patterns
                    .iter()
                    .map(|e| ExcludesForFolder { base_uri: None, patterns: resolve_patterns_for_provider(common.exclude_pattern.as_ref(), Some(&e.pattern)) })
                    .collect()
            })
            .unwrap_or_default();

        if exclude_pattern.is_empty() {
            exclude_pattern = vec![ExcludesForFolder { base_uri: None, patterns: resolve_patterns_for_provider(common.exclude_pattern.as_ref(), None) }];
        }
        let excludes = exclude_to_glob_pattern(&exclude_pattern);

        let encoding = match fq.file_encoding.as_deref() {
            Some(file_encoding) if !file_encoding.is_empty() => (self.file_utils.to_canonical_name)(file_encoding),
            _ => String::new(),
        };

        TextSearchProviderFolderOptions {
            folder: fq.folder.clone(),
            excludes,
            includes,
            use_ignore_files: UseIgnoreFiles {
                local: !fq.disregard_ignore_files.unwrap_or(false),
                parent: !fq.disregard_parent_ignore_files.unwrap_or(false),
                global: !fq.disregard_global_ignore_files.unwrap_or(false),
            },
            follow_symlinks: !fq.ignore_symlinks.unwrap_or(false),
            encoding,
            ignore_glob_case: if common.ignore_glob_case.unwrap_or(false) { Some(true) } else { fq.ignore_glob_case },
        }
    }
}

/// `patternInfoToQuery`.
fn pattern_info_to_query(pattern_info: &PatternInfo) -> TextSearchQuery2 {
    TextSearchQuery2 {
        is_case_sensitive: Some(pattern_info.is_case_sensitive.unwrap_or(false)),
        is_reg_exp: Some(pattern_info.is_reg_exp.unwrap_or(false)),
        is_word_match: Some(pattern_info.is_word_match.unwrap_or(false)),
        is_multiline: Some(pattern_info.is_multiline.unwrap_or(false)),
        pattern: pattern_info.pattern.clone(),
    }
}

/// `TextSearchResultsCollector`.
pub struct TextSearchResultsCollector {
    batched_collector: BatchedCollector<IFileMatch>,

    /// `-1` do original é `None`.
    current_folder_idx: Option<usize>,
    current_uri: Option<URI>,
    current_file_match: Option<IFileMatch>,
}

impl TextSearchResultsCollector {
    pub fn new(on_result: Rc<dyn Fn(Vec<IFileMatch>)>) -> Self {
        Self { batched_collector: BatchedCollector::new(512, on_result), current_folder_idx: None, current_uri: None, current_file_match: None }
    }

    pub fn add(&mut self, data: TextSearchResult2, folder_idx: usize) {
        // Collects TextSearchResults into IInternalFileMatches and collates using BatchedCollector.
        // This is efficient for ripgrep which sends results back one file at a time. It wouldn't be efficient for other search
        // providers that send results in random order. We could do this step afterwards instead.
        let uri = text_search_result_uri(&data);
        if self.current_file_match.is_some() && (self.current_folder_idx != Some(folder_idx) || !ext_uri().is_equal(self.current_uri.as_ref(), Some(uri), false)) {
            self.push_to_collector();
            self.current_file_match = None;
        }

        if self.current_file_match.is_none() {
            self.current_folder_idx = Some(folder_idx);
            self.current_uri = Some(uri.clone());
            self.current_file_match = Some(IFileMatch::new(uri.clone(), Some(Vec::new())));
        }

        if let Some(results) = self.current_file_match.as_mut().and_then(|file_match| file_match.results.as_mut()) {
            results.push(extension_result_to_frontend_result(&data));
        }
    }

    fn push_to_collector(&mut self) {
        let size = self.current_file_match.as_ref().and_then(|file_match| file_match.results.as_ref()).map_or(0, Vec::len);
        // O `addItem` do original ignora o item nulo.
        if let Some(file_match) = self.current_file_match.clone() {
            self.batched_collector.add_item(file_match, size);
        }
    }

    pub fn flush(&mut self) {
        self.push_to_collector();
        self.batched_collector.flush();
    }
}

/// `extensionResultToFrontendResult`.
fn extension_result_to_frontend_result(data: &TextSearchResult2) -> ITextSearchResult {
    // Warning: result from RipgrepTextSearchEH has fake Range. Don't depend on any other props beyond these...
    match data {
        TextSearchResult2::Match(text_match) => ITextSearchResult::Match(TextSearchMatch {
            preview_text: text_match.preview_text.clone(),
            range_locations: text_match
                .ranges
                .iter()
                .map(|r| SearchRangeSetPairing {
                    preview: SearchRange::new(r.preview_range.start.line, r.preview_range.start.character, r.preview_range.end.line, r.preview_range.end.character),
                    source: SearchRange::new(r.source_range.start.line, r.source_range.start.character, r.source_range.end.line, r.source_range.end.character),
                })
                .collect(),
            webview_index: None,
        }),
        TextSearchResult2::Context(context) => ITextSearchResult::Context(ITextSearchContext { text: context.text.clone(), line_number: context.line_number }),
    }
}

const BATCHED_COLLECTOR_TIMEOUT: u64 = 4000;

/// After START_BATCH_AFTER_COUNT items have been collected, stop flushing on timeout.
const BATCHED_COLLECTOR_START_BATCH_AFTER_COUNT: usize = 50;

struct BatchedCollectorState<T> {
    max_batch_size: usize,
    cb: Rc<dyn Fn(Vec<T>)>,
    total_number_completed: Cell<usize>,
    batch: RefCell<Vec<T>>,
    batch_size: Cell<usize>,
    timeout_handle: Cell<Option<TimerId>>,
}

impl<T: 'static> BatchedCollectorState<T> {
    fn on_update(self: &Rc<Self>) {
        if self.total_number_completed.get() < BATCHED_COLLECTOR_START_BATCH_AFTER_COUNT {
            // Flush because we aren't batching yet
            self.flush();
        } else if self.batch_size.get() >= self.max_batch_size {
            // Flush because the batch is full
            self.flush();
        } else if self.timeout_handle.get().is_none() {
            // No timeout running, start a timeout to flush
            let state = self.clone();
            self.timeout_handle.set(Some(runtime::set_timeout(move || state.flush(), BATCHED_COLLECTOR_TIMEOUT)));
        }
    }

    fn flush(self: &Rc<Self>) {
        if self.batch_size.get() != 0 {
            self.total_number_completed.set(self.total_number_completed.get() + self.batch_size.get());
            let batch = self.batch.replace(Vec::new());
            (self.cb)(batch);
            self.batch_size.set(0);

            if let Some(timeout_handle) = self.timeout_handle.take() {
                runtime::clear_timeout(timeout_handle);
            }
        }
    }
}

/// Collects items that have a size - before the cumulative size of collected items reaches START_BATCH_AFTER_COUNT, the callback is called for every
/// set of items collected.
/// But after that point, the callback is called with batches of maxBatchSize.
/// If the batch isn't filled within some time, the callback is also called.
pub struct BatchedCollector<T: 'static> {
    state: Rc<BatchedCollectorState<T>>,
}

impl<T: 'static> BatchedCollector<T> {
    pub fn new(max_batch_size: usize, cb: Rc<dyn Fn(Vec<T>)>) -> Self {
        Self {
            state: Rc::new(BatchedCollectorState {
                max_batch_size,
                cb,
                total_number_completed: Cell::new(0),
                batch: RefCell::new(Vec::new()),
                batch_size: Cell::new(0),
                timeout_handle: Cell::new(None),
            }),
        }
    }

    pub fn add_item(&self, item: T, size: usize) {
        self.state.batch.borrow_mut().push(item);
        self.state.batch_size.set(self.state.batch_size.get() + size);
        self.state.on_update();
    }

    pub fn add_items(&self, items: Vec<T>, size: usize) {
        self.state.batch.borrow_mut().extend(items);
        self.state.batch_size.set(self.state.batch_size.get() + size);
        self.state.on_update();
    }

    pub fn flush(&self) {
        self.state.flush();
    }
}

/// `NativeTextSearchManager` (`node/textSearchManager.ts`).
pub struct NativeTextSearchManager;

impl NativeTextSearchManager {
    /// `new NativeTextSearchManager(query, provider)`: o `readdir` é o `Promises.readdir` do disco e o tipo de
    /// processo é `searchProcess`.
    pub fn new(query: ITextQuery, provider: Rc<dyn TextSearchProvider2>) -> TextSearchManager {
        Self::with_readdir(query, provider, Rc::new(Self::disk_readdir), TextSearchStatsType::SearchProcess)
    }

    /// `new NativeTextSearchManager(query, provider, _pfs, processType)`.
    pub fn with_readdir(query: ITextQuery, provider: Rc<dyn TextSearchProvider2>, readdir: Rc<dyn Fn(&str) -> Promise<Vec<String>>>, process_type: TextSearchStatsType) -> TextSearchManager {
        TextSearchManager::new(
            TextQueryProviderPair::Text { query, provider },
            IFileUtils {
                readdir: Rc::new(move |resource: &URI| readdir(&je_platform::resources::fs_path(resource))),
                to_canonical_name: Rc::new(to_canonical_name),
            },
            process_type,
        )
    }

    /// `pfs.Promises.readdir(path)`.
    pub fn disk_readdir(path: &str) -> Promise<Vec<String>> {
        match pfs::readdir_names(path) {
            Ok(names) => Promise::resolved(names),
            Err(error) => Promise::rejected(Error::from(error)),
        }
    }
}
