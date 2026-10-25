//! Porte de `src/vs/workbench/services/search/node/rawSearchService.ts`: o `SearchService` do processo de busca. Ele roda a
//! busca de arquivo (o `Engine` de `file_walker`, com o ripgrep) em lotes, com a busca ordenada por pontuação fuzzy e o
//! cache por `cacheKey`, e a busca de texto (o [`TextSearchEngineAdapter`]), e entrega o progresso como eventos.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `fileSearch`/`textSearch` devolvem um [`Event`] de [`ISerializedSearchEvent`] (o item de progresso ou o
//!   `ISerializedSearchComplete`), pelo trait [`IRawSearchService`]. O `reviveQuery` é a identidade: as consultas já chegam
//!   com `URI`. O `Emitter` com `onDidAddFirstListener` e `onDidRemoveLastListener` é o [`raw_search_event`], comum às duas.
//! - `new SearchService(processType = 'searchProcess', getNumThreads?)` é [`SearchService::new`] (um `Rc`, porque os eventos
//!   guardam o serviço). `EngineClass` (um construtor `new(config, numThreads)`) é o [`EngineClass`], um fecho que devolve o
//!   motor; o `FileSearchEngine` padrão é o `Engine` de `file_walker` sobre o `rg` de [`rg_disk_path`].
//! - `IProgressCallback` e `IFileProgressCallback` são fechos `Rc<dyn Fn>`; `token?: CancellationToken` é `Option`.
//! - `Cache.resultsToSearchCache` é um objeto cujas chaves o `for ... in` percorre com os índices inteiros primeiro (em
//!   ordem crescente) e as demais na ordem de inserção: [`Cache::rows_in_property_order`] reproduz essa ordem. O `resolved`
//!   da linha é compartilhado (`Rc<Cell<bool>>`) porque a promessa o marca depois, e a cópia que o `getResultsFromCache`
//!   faz guarda o valor do momento.
//! - `preventCancellation` é uma [`CancelablePromise`] cujo `cancel` não faz nada.
//! - `arrays.topAsync` é o `top_async` de `je_base::arrays`; `FileMatchItemAccessor` é o [`ItemAccessor`] abaixo.
//! - `JSON.stringify(complete.stats)` é o `json_stringify` do porte sobre um objeto com as chaves na ordem do original.
//! - Um `IRawFileMatch` sem `relativePath` (falso no JavaScript) que chega como item de progresso é, como no original,
//!   tratado como mensagem (aqui, uma mensagem vazia).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::arrays::top_async;
use je_base::async_::{async_fn, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::Error;
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::fuzzy_scorer::{FuzzyScorerCache, ItemAccessor, compare_items_by_fuzzy_score, prepare_query};
use je_base::js_string::JsString;
use je_base::lifecycle::IDisposable;
use je_base::platform::Platform;
use je_base::promise::{CancelablePromise, Promise};
use je_base::stopwatch::StopWatch;
use je_platform::files::files::byte_size;

use super::file_walker::{Engine, ISearchEngine, ISearchEngineSuccess, with_native_path};
use super::ripgrep_disk_path::rg_disk_path;
use super::ripgrep_search_provider::GetNumThreadsFn;
use super::search::{
    DEFAULT_MAX_SEARCH_RESULTS, FileSearchDetailStats, FileSearchStatsType, ICachedSearchStats, IFileQuery, IFileSearchProgressItem, IFileSearchStats, IProgressMessage,
    IRawFileMatch, IRawSearchService, ISearchEngineStats, ISearchStats, ISerializedFileMatch, ISerializedSearchComplete, ISerializedSearchError, ISerializedSearchEvent,
    ISerializedSearchProgressItem, ISerializedSearchSuccess, ITextQuery, is_file_pattern_match,
};
use super::file_walker::spawn_ripgrep_cmd;
use super::text_search_adapter::TextSearchEngineAdapter;

/// `IProgressCallback`.
pub type IProgressCallback = Rc<dyn Fn(ISerializedSearchProgressItem)>;

/// `IFileProgressCallback`.
type IFileProgressCallback = Rc<dyn Fn(IFileSearchProgressItem)>;

/// O `EngineClass` de `doFileSearchWithEngine`: `new(config, numThreads)`.
pub type EngineClass = Rc<dyn Fn(&IFileQuery, Option<i64>) -> Rc<dyn ISearchEngine<IRawFileMatch>>>;

/// O resultado de uma busca ordenada: o que `doSortedSearch` e `trySortedSearchFromCache` entregam.
type SortedSearch = (ISerializedSearchSuccess, Vec<IRawFileMatch>);

const BATCH_SIZE: i64 = 512;

/// `ICacheRow`.
#[derive(Clone)]
struct CacheRow {
    // TODO@roblou do original: never actually canceled
    promise: CancelablePromise<(ISearchEngineSuccess, Vec<IRawFileMatch>)>,
    resolved: Rc<Cell<bool>>,
    event: Event<IFileSearchProgressItem>,
}

/// `Cache`.
#[derive(Default)]
struct Cache {
    results_to_search_cache: RefCell<Vec<(String, CacheRow)>>,
    scorer_cache: Rc<RefCell<FuzzyScorerCache>>,
}

/// Se a chave é um índice de array (inteiro canônico entre 0 e 2^32 - 2), que o JavaScript enumera primeiro.
fn array_index_key(key: &str) -> Option<u32> {
    let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
    if !canonical || !key.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|index| *index != u32::MAX)
}

impl Cache {
    /// `cache.resultsToSearchCache[key] = row`: uma chave que já existe mantém a posição.
    fn set_row(&self, key: String, row: CacheRow) {
        let mut rows = self.results_to_search_cache.borrow_mut();
        match rows.iter_mut().find(|(existing, _)| *existing == key) {
            Some(slot) => slot.1 = row,
            None => rows.push((key, row)),
        }
    }

    /// `delete cache.resultsToSearchCache[key]`.
    fn remove_row(&self, key: &str) {
        self.results_to_search_cache.borrow_mut().retain(|(existing, _)| existing != key);
    }

    /// As linhas na ordem do `for (const previousSearch in cache.resultsToSearchCache)`.
    fn rows_in_property_order(&self) -> Vec<(String, CacheRow)> {
        let rows = self.results_to_search_cache.borrow();
        let mut indexed: Vec<(u32, &(String, CacheRow))> = rows.iter().filter_map(|row| array_index_key(&row.0).map(|index| (index, row))).collect();
        indexed.sort_by_key(|(index, _)| *index);
        let others = rows.iter().filter(|row| array_index_key(&row.0).is_none());
        indexed.into_iter().map(|(_, row)| row).chain(others).cloned().collect()
    }
}

/// `FileMatchItemAccessor`.
struct FileMatchItemAccessor;

impl ItemAccessor<IRawFileMatch> for FileMatchItemAccessor {
    fn get_item_label(&self, item: &IRawFileMatch) -> Option<JsString> {
        // e.g. myFile.txt
        Some(JsString::from(with_native_path(|path| path.basename(&item.relative_path, None))))
    }

    fn get_item_description(&self, item: &IRawFileMatch) -> Option<JsString> {
        // e.g. some/path/to/file
        Some(JsString::from(with_native_path(|path| path.dirname(&item.relative_path))))
    }

    fn get_item_path(&self, file: &IRawFileMatch) -> Option<JsString> {
        // e.g. some/path/to/file/myFile.txt
        Some(JsString::from(file.relative_path.as_str()))
    }
}

/// `rawMatchToSearchItem`.
fn raw_match_to_search_item(raw_match: &IRawFileMatch) -> ISerializedFileMatch {
    let path = match raw_match.base.as_deref() {
        Some(base) if !base.is_empty() => with_native_path(|path| path.join(&[base, raw_match.relative_path.as_str()])),
        _ => raw_match.relative_path.clone(),
    };
    ISerializedFileMatch::new(path)
}

/// `preventCancellation`: a mesma promessa, cujo `cancel` não faz nada.
fn prevent_cancellation<C: Clone + 'static>(promise: &CancelablePromise<C>) -> CancelablePromise<C> {
    CancelablePromise::new((**promise).clone(), || {})
}

/// `sendProgress`.
fn send_progress(results: Vec<ISerializedFileMatch>, progress_cb: &IProgressCallback, batch_size: i64) {
    if batch_size > 0 {
        for chunk in results.chunks(batch_size as usize) {
            progress_cb(ISerializedSearchProgressItem::FileMatches(chunk.to_vec()));
        }
    } else {
        progress_cb(ISerializedSearchProgressItem::FileMatches(results));
    }
}

/// `sortResults`.
fn sort_results(config: &IFileQuery, results: Vec<IRawFileMatch>, scorer_cache: Rc<RefCell<FuzzyScorerCache>>, token: Option<&CancellationToken>) -> Promise<Vec<IRawFileMatch>> {
    // we use the same compare function that is used later when showing the results using fuzzy scoring
    // this is very important because we are also limiting the number of results by config.maxResults
    // and as such we want the top items to be included in this result set if the number of items
    // exceeds config.maxResults.
    let platform = Platform::detect();
    let file_pattern: Vec<u16> = config.file_pattern.as_deref().unwrap_or("").encode_utf16().collect();
    let query = prepare_query(&file_pattern, &platform);
    let compare = move |match_a: &IRawFileMatch, match_b: &IRawFileMatch| {
        f64::from(compare_items_by_fuzzy_score(match_a, match_b, &query, true, &FileMatchItemAccessor, &mut scorer_cache.borrow_mut(), &platform))
    };

    let max_results = config.common.max_results.map_or(DEFAULT_MAX_SEARCH_RESULTS, |max_results| max_results.max(0) as usize);
    top_async(results, compare, max_results, 10000, token.cloned())
}

/// O `JSON.stringify(complete.stats)`, com as chaves na ordem de `FileWalker.getStats`.
fn stats_to_json(stats: &ISearchEngineStats) -> String {
    let mut entries = vec![
        ("cmdTime", Dynamic::Number(stats.cmd_time)),
        ("fileWalkTime", Dynamic::Number(stats.file_walk_time)),
        ("directoriesWalked", Dynamic::Number(stats.directories_walked)),
        ("filesWalked", Dynamic::Number(stats.files_walked)),
    ];
    if let Some(cmd_result_count) = stats.cmd_result_count {
        entries.push(("cmdResultCount", Dynamic::Number(cmd_result_count)));
    }
    json_stringify(&Dynamic::object(entries)).unwrap_or_default()
}

/// `doSearch`.
fn do_search(engine: Rc<dyn ISearchEngine<IRawFileMatch>>, progress_callback: IFileProgressCallback, batch_size: i64, token: Option<&CancellationToken>) -> Promise<ISearchEngineSuccess> {
    let (promise, resolver) = Promise::new();
    let batch: Rc<RefCell<Vec<IRawFileMatch>>> = Rc::default();
    if let Some(token) = token {
        let engine = engine.clone();
        // O ouvinte fica pendurado, como no original: o token vive tanto quanto a busca.
        token.on_cancellation_requested().listen(move |_| engine.cancel());
    }

    let on_result: Rc<dyn Fn(&IRawFileMatch)> = {
        let (batch, progress_callback) = (batch.clone(), progress_callback.clone());
        Rc::new(move |raw_match: &IRawFileMatch| {
            if batch_size != 0 {
                batch.borrow_mut().push(raw_match.clone());
                if batch_size > 0 && batch.borrow().len() >= batch_size as usize {
                    let full = std::mem::take(&mut *batch.borrow_mut());
                    progress_callback(IFileSearchProgressItem::Matches(full));
                }
            } else {
                progress_callback(IFileSearchProgressItem::Match(raw_match.clone()));
            }
        })
    };
    let on_progress: Rc<dyn Fn(&IProgressMessage)> = {
        let progress_callback = progress_callback.clone();
        Rc::new(move |progress: &IProgressMessage| progress_callback(IFileSearchProgressItem::Message(progress.clone())))
    };
    let done: Box<dyn FnOnce(Option<Error>, ISearchEngineSuccess)> = Box::new(move |error, complete| {
        let remaining = std::mem::take(&mut *batch.borrow_mut());
        if !remaining.is_empty() {
            progress_callback(IFileSearchProgressItem::Matches(remaining));
        }

        match error {
            Some(error) => {
                progress_callback(IFileSearchProgressItem::Message(IProgressMessage { message: format!("Search finished. Error: {}", error.message) }));
                resolver.reject(error);
            }
            None => {
                progress_callback(IFileSearchProgressItem::Message(IProgressMessage { message: format!("Search finished. Stats: {}", stats_to_json(&complete.stats)) }));
                resolver.resolve(complete);
            }
        }
    });
    engine.search(on_result, on_progress, done);
    promise
}

/// `new Emitter({ onDidAddFirstListener, onDidRemoveLastListener })` de `fileSearch` e `textSearch`: a busca começa quando o
/// primeiro ouvinte entra, entrega o progresso e o resultado (ou o erro) e se cancela quando o último sai.
fn raw_search_event(run: impl Fn(CancellationToken, IProgressCallback) -> Promise<ISerializedSearchSuccess> + 'static) -> Event<ISerializedSearchEvent> {
    let slot: Rc<RefCell<Weak<Emitter<ISerializedSearchEvent>>>> = Rc::default();
    let running: Rc<RefCell<Option<CancelablePromise<ISerializedSearchSuccess>>>> = Rc::default();

    let on_first_listener = {
        let (slot, running) = (slot.clone(), running.clone());
        move |_: &Emitter<ISerializedSearchEvent>| {
            let Some(emitter) = slot.borrow().upgrade() else {
                return;
            };
            let progress_emitter = emitter.clone();
            let promise = create_cancelable_promise(|token| {
                let on_progress: IProgressCallback = Rc::new(move |progress| progress_emitter.fire(ISerializedSearchEvent::Progress(progress)));
                run(token, on_progress)
            });
            *running.borrow_mut() = Some(promise.clone());

            promise.then_result(move |result| {
                emitter.fire(ISerializedSearchEvent::Complete(match result {
                    Ok(complete) => ISerializedSearchComplete::Success(complete),
                    Err(error) => ISerializedSearchComplete::Error(ISerializedSearchError { message: error.message, stack: error.stack }),
                }));
                Ok(())
            });
        }
    };
    let on_last_listener_removed = move |_: &Emitter<ISerializedSearchEvent>| {
        if let Some(promise) = running.borrow().as_ref() {
            promise.cancel();
        }
    };

    let emitter = Emitter::with_options(EmitterOptions {
        on_did_add_first_listener: Some(Rc::new(on_first_listener)),
        on_did_remove_last_listener: Some(Rc::new(on_last_listener_removed)),
        ..EmitterOptions::default()
    });
    *slot.borrow_mut() = Rc::downgrade(&emitter);
    emitter.event()
}

/// `SearchService`.
pub struct SearchService {
    this: Weak<SearchService>,
    process_type: FileSearchStatsType,
    get_num_threads: Option<GetNumThreadsFn>,
    caches: RefCell<HashMap<String, Rc<Cache>>>,
    file_search_engine: EngineClass,
}

impl SearchService {
    /// `new SearchService(processType, getNumThreads)`: o motor de busca de arquivo é o `Engine` sobre o ripgrep.
    pub fn new(process_type: FileSearchStatsType, get_num_threads: Option<GetNumThreadsFn>) -> Rc<Self> {
        let file_search_engine: EngineClass = Rc::new(|config: &IFileQuery, num_threads: Option<i64>| {
            Rc::new(Engine::new(config, num_threads, spawn_ripgrep_cmd(rg_disk_path()))) as Rc<dyn ISearchEngine<IRawFileMatch>>
        });
        Rc::new_cyclic(|this| SearchService { this: this.clone(), process_type, get_num_threads, caches: RefCell::default(), file_search_engine })
    }

    fn service(&self) -> Rc<SearchService> {
        self.this.upgrade().expect("o serviço está vivo enquanto um método dele roda")
    }

    /// `await this.getNumThreads?.()`.
    fn num_threads(&self) -> Promise<Option<i64>> {
        match &self.get_num_threads {
            Some(get_num_threads) => get_num_threads(),
            None => Promise::resolved(None),
        }
    }

    /// `ripgrepTextSearch`.
    fn ripgrep_text_search(&self, mut config: ITextQuery, progress_callback: IProgressCallback, token: CancellationToken) -> Promise<ISerializedSearchSuccess> {
        config.max_file_size = Some(Self::get_platform_file_limits());
        let num_threads = self.num_threads();
        async_fn(async move {
            let num_threads = num_threads.await?;
            let engine = TextSearchEngineAdapter::new(config, num_threads);

            let on_result: Rc<dyn Fn(Vec<ISerializedFileMatch>)> = {
                let progress_callback = progress_callback.clone();
                Rc::new(move |matches| progress_callback(ISerializedSearchProgressItem::FileMatches(matches)))
            };
            let on_message: Rc<dyn Fn(IProgressMessage)> = Rc::new(move |message| progress_callback(ISerializedSearchProgressItem::Message(message)));
            engine.search(&token, on_result, on_message).await
        })
    }

    /// `getPlatformFileLimits().maxFileSize`.
    fn get_platform_file_limits() -> i64 {
        (16 * byte_size::GB) as i64
    }

    pub fn do_file_search(&self, config: &IFileQuery, num_threads: Option<i64>, progress_callback: IProgressCallback, token: Option<&CancellationToken>) -> Promise<ISerializedSearchSuccess> {
        self.do_file_search_with_engine(&self.file_search_engine, config, progress_callback, token, BATCH_SIZE, num_threads)
    }

    pub fn do_file_search_with_engine(
        &self,
        engine_class: &EngineClass,
        config: &IFileQuery,
        progress_callback: IProgressCallback,
        token: Option<&CancellationToken>,
        batch_size: i64,
        threads: Option<i64>,
    ) -> Promise<ISerializedSearchSuccess> {
        let result_count = Rc::new(Cell::new(0_usize));
        let file_progress_callback: IFileProgressCallback = {
            let (result_count, progress_callback) = (result_count.clone(), progress_callback.clone());
            Rc::new(move |progress: IFileSearchProgressItem| match progress {
                IFileSearchProgressItem::Matches(matches) => {
                    result_count.set(result_count.get() + matches.len());
                    progress_callback(ISerializedSearchProgressItem::FileMatches(matches.iter().map(raw_match_to_search_item).collect()));
                }
                IFileSearchProgressItem::Match(raw_match) if !raw_match.relative_path.is_empty() => {
                    result_count.set(result_count.get() + 1);
                    progress_callback(ISerializedSearchProgressItem::FileMatch(raw_match_to_search_item(&raw_match)));
                }
                IFileSearchProgressItem::Match(_) => progress_callback(ISerializedSearchProgressItem::Message(IProgressMessage { message: String::new() })),
                IFileSearchProgressItem::Message(message) => progress_callback(ISerializedSearchProgressItem::Message(message)),
            })
        };

        if config.sort_by_score.unwrap_or(false) {
            let sorted_search = self.try_sorted_search_from_cache(config, &file_progress_callback, token).unwrap_or_else(|| {
                let mut walker_config = config.clone();
                if config.common.max_results.is_some_and(|max_results| max_results != 0) {
                    walker_config.common.max_results = None;
                }
                let engine = engine_class(&walker_config, threads);
                self.do_sorted_search(engine, config, file_progress_callback.clone(), token)
            });

            return sorted_search.then(move |(result, raw_matches)| {
                let serialized_matches = raw_matches.iter().map(raw_match_to_search_item).collect();
                send_progress(serialized_matches, &progress_callback, batch_size);
                result
            });
        }

        let engine = engine_class(config, threads);
        let process_type = self.process_type;

        do_search(engine, file_progress_callback, batch_size, token).then(move |complete| ISerializedSearchSuccess {
            limit_hit: complete.limit_hit,
            stats: Some(ISearchStats::File(IFileSearchStats {
                detail_stats: FileSearchDetailStats::Engine(complete.stats),
                stats_type: process_type,
                from_cache: false,
                result_count: result_count.get() as f64,
                sorting_time: None,
            })),
            messages: Vec::new(),
        })
    }

    /// `doSortedSearch`.
    fn do_sorted_search(
        &self,
        engine: Rc<dyn ISearchEngine<IRawFileMatch>>,
        config: &IFileQuery,
        file_progress_callback: IFileProgressCallback,
        token: Option<&CancellationToken>,
    ) -> Promise<SortedSearch> {
        let emitter: Rc<Emitter<IFileSearchProgressItem>> = Emitter::new();

        let mut all_results_promise = create_cancelable_promise({
            let emitter = emitter.clone();
            move |token| {
                let results: Rc<RefCell<Vec<IRawFileMatch>>> = Rc::default();

                let inner_progress_callback: IFileProgressCallback = {
                    let results = results.clone();
                    Rc::new(move |progress: IFileSearchProgressItem| match progress {
                        IFileSearchProgressItem::Matches(matches) => *results.borrow_mut() = matches,
                        progress => {
                            file_progress_callback(progress.clone());
                            emitter.fire(progress);
                        }
                    })
                };

                do_search(engine, inner_progress_callback, -1, Some(&token)).then(move |result| (result, results.borrow().clone()))
            }
        });

        let cache = config.cache_key.as_deref().filter(|cache_key| !cache_key.is_empty()).map(|cache_key| {
            let cache = self.get_or_create_cache(cache_key);
            let file_pattern = config.file_pattern.clone().unwrap_or_default();
            let cache_row = CacheRow { promise: all_results_promise.clone(), event: emitter.event(), resolved: Rc::new(Cell::new(false)) };
            cache.set_row(file_pattern.clone(), cache_row.clone());
            let (settled_cache, resolved) = (cache.clone(), cache_row.resolved.clone());
            all_results_promise.then_result(move |result| {
                match result {
                    Ok(_) => resolved.set(true),
                    Err(_) => settled_cache.remove_row(&file_pattern),
                }
                Ok(())
            });

            all_results_promise = prevent_cancellation(&all_results_promise);
            cache
        });

        let scorer_cache = cache.map(|cache| cache.scorer_cache.clone()).unwrap_or_default();
        let (config, token, process_type) = (config.clone(), token.cloned(), self.process_type);
        all_results_promise.then_promise(move |(result, results)| {
            let max_results = config.common.max_results;
            let sort_sw = max_results.is_none_or(|max_results| max_results > 0).then(|| StopWatch::create(Some(false)));
            sort_results(&config, results.clone(), scorer_cache, token.as_ref()).then(move |sorted_results| {
                // sortingTime: -1 indicates a "sorted" search that was not sorted, i.e. populating the cache when quickaccess is opened.
                // Contrasting with findFiles which is not sorted and will have sortingTime: undefined
                let sorting_time = sort_sw.map_or(-1.0, |sort_sw| sort_sw.elapsed());

                (
                    ISerializedSearchSuccess {
                        stats: Some(ISearchStats::File(IFileSearchStats {
                            detail_stats: FileSearchDetailStats::Engine(result.stats),
                            sorting_time: Some(sorting_time),
                            from_cache: false,
                            stats_type: process_type,
                            result_count: sorted_results.len() as f64,
                        })),
                        messages: result.messages,
                        limit_hit: result.limit_hit || max_results.is_some_and(|max_results| results.len() as i64 > max_results),
                    },
                    sorted_results,
                )
            })
        })
    }

    fn get_or_create_cache(&self, cache_key: &str) -> Rc<Cache> {
        self.caches.borrow_mut().entry(cache_key.to_owned()).or_default().clone()
    }

    /// `trySortedSearchFromCache`.
    fn try_sorted_search_from_cache(&self, config: &IFileQuery, progress_callback: &IFileProgressCallback, token: Option<&CancellationToken>) -> Option<Promise<SortedSearch>> {
        let cache = config.cache_key.as_deref().filter(|cache_key| !cache_key.is_empty()).and_then(|cache_key| self.caches.borrow().get(cache_key).cloned())?;

        let cached = Self::get_results_from_cache(&cache, config.file_pattern.as_deref().unwrap_or(""), progress_callback, token)?;
        let (config, token, process_type) = (config.clone(), token.cloned(), self.process_type);
        Some(cached.then_promise(move |(result, results, cache_stats)| {
            let sort_sw = StopWatch::create(Some(false));
            let scorer_cache = cache.scorer_cache.clone();
            sort_results(&config, results.clone(), scorer_cache, token.as_ref()).then(move |sorted_results| {
                let sorting_time = sort_sw.elapsed();
                let stats = IFileSearchStats {
                    from_cache: true,
                    detail_stats: FileSearchDetailStats::Cached(cache_stats),
                    stats_type: process_type,
                    result_count: results.len() as f64,
                    sorting_time: Some(sorting_time),
                };

                (
                    ISerializedSearchSuccess {
                        limit_hit: result.limit_hit || config.common.max_results.is_some_and(|max_results| results.len() as i64 > max_results),
                        stats: Some(ISearchStats::File(stats)),
                        messages: Vec::new(),
                    },
                    sorted_results,
                )
            })
        }))
    }

    /// `getResultsFromCache`.
    fn get_results_from_cache(
        cache: &Cache,
        search_value: &str,
        progress_callback: &IFileProgressCallback,
        token: Option<&CancellationToken>,
    ) -> Option<Promise<(ISearchEngineSuccess, Vec<IRawFileMatch>, ICachedSearchStats)>> {
        let cache_lookup_sw = StopWatch::create(Some(false));

        // Find cache entries by prefix of search value
        let sep = std::path::MAIN_SEPARATOR;
        let has_path_sep = search_value.contains(sep);
        let mut cached_row: Option<(CancelablePromise<(ISearchEngineSuccess, Vec<IRawFileMatch>)>, Event<IFileSearchProgressItem>, bool)> = None;
        for (previous_search, row) in cache.rows_in_property_order() {
            // If we narrow down, we might be able to reuse the cached results
            if search_value.starts_with(&previous_search) {
                if has_path_sep && !previous_search.contains(sep) && !previous_search.is_empty() {
                    continue; // since a path character widens the search for potential more matches, require it in previous search too
                }

                cached_row = Some((prevent_cancellation(&row.promise), row.event.clone(), row.resolved.get()));
                break;
            }
        }

        let (cached_promise, cached_event, cached_resolved) = cached_row?;

        let cache_lookup_time = cache_lookup_sw.elapsed();
        let cache_filter_sw = StopWatch::create(Some(false));

        let listener = {
            let progress_callback = progress_callback.clone();
            cached_event.listen(move |progress| progress_callback(progress.clone()))
        };
        if let Some(token) = token {
            let listener = RefCell::new(Some(listener));
            token.on_cancellation_requested().listen(move |_| {
                if let Some(listener) = listener.borrow_mut().take() {
                    listener.dispose();
                }
            });
        }

        let (search_value, token) = (search_value.to_owned(), token.cloned());
        Some(cached_promise.then_result(move |result| {
            let (complete, cached_entries) = result?;
            if token.as_ref().is_some_and(|token| token.is_cancellation_requested()) {
                return Err(Error::cancellation());
            }

            // Pattern match on results
            let normalized_search_value_lowercase = {
                let units: Vec<u16> = search_value.encode_utf16().collect();
                prepare_query(&units, &Platform::detect()).normalized_lowercase.to_string_lossy()
            };
            let results: Vec<IRawFileMatch> =
                cached_entries.iter().filter(|entry| is_file_pattern_match(entry, &normalized_search_value_lowercase, true, false)).cloned().collect();

            Ok((
                complete,
                results,
                ICachedSearchStats {
                    cache_was_resolved: cached_resolved,
                    cache_lookup_time,
                    cache_filter_time: cache_filter_sw.elapsed(),
                    cache_entry_count: cached_entries.len() as f64,
                },
            ))
        }))
    }
}

impl IRawSearchService for SearchService {
    fn file_search(&self, config: IFileQuery) -> Event<ISerializedSearchEvent> {
        let service = self.service();
        raw_search_event(move |token, progress_callback| {
            let (service, query) = (service.clone(), config.clone());
            let num_threads = service.num_threads();
            async_fn(async move {
                let num_threads = num_threads.await?;
                service.do_file_search_with_engine(&service.file_search_engine, &query, progress_callback, Some(&token), BATCH_SIZE, num_threads).await
            })
        })
    }

    fn text_search(&self, raw_query: ITextQuery) -> Event<ISerializedSearchEvent> {
        let service = self.service();
        raw_search_event(move |token, progress_callback| service.ripgrep_text_search(raw_query.clone(), progress_callback, token))
    }

    fn clear_cache(&self, cache_key: &str) -> Promise<()> {
        self.caches.borrow_mut().remove(cache_key);
        Promise::resolved(())
    }
}
