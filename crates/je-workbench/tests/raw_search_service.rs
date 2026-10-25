//! Porte de `src/vs/workbench/services/search/test/node/rawSearchService.integrationTest.ts` (suíte `RawSearchService`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `TestSearchEngine.bind(null, () => ...)` é o [`EngineClass`] que [`test_engine`] devolve: o fecho de resultados é
//!   compartilhado entre as construções, como o `bind` faz, e `TestSearchEngine.last` é o [`LAST_CONFIG`]. `process.nextTick`
//!   é `queue_microtask`.
//! - `i-- ? rawMatch : null` é o fecho que lê `i` antes de decrementar; `matches.shift()` é `VecDeque::pop_front`.
//! - `assert.deepStrictEqual(value, match)` é `assert_eq!` sobre o [`ISerializedFileMatch`]; `assert.fail(...)` é `panic!`.
//! - `batchSize` de `0`, `-1` e `undefined` do original são `0`, `-1` e `None`/`0`; `token` `null`/`undefined` é `None`.
//! - O `fileSearch` do caso "Collect batched results" é o mesmo `Emitter` do original, montado com os ganchos
//!   `on_will_add_first_listener` e `on_did_remove_last_listener`; o `Event` sobre os `Emitter`s é o mesmo que o
//!   `IRawSearchService` devolve.
//! - Os três casos de várias raízes rodam o `rg` de verdade sobre `tests/fixtures/search/{examples,more}`: o binário é o
//!   do oráculo (`ref/oracle`), registrado em `set_rg_disk_path`, ou o `rg` do `PATH`.
//! - O `try { ... } catch (e) { }` do caso "Cached results" engole as asserções da segunda busca, e o porte também.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::rc::{Rc, Weak};

use common::run_with_real_timers;
use je_base::async_::create_cancelable_promise;
use je_base::errors::Error;
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::glob::{ExprValue, Expression};
use je_base::lifecycle::DisposableRef;
use je_base::promise::{CancelablePromise, Promise};
use je_base::runtime;
use je_platform::resources::uri_file;
use je_workbench::services::search::file_walker::{ISearchEngine, ISearchEngineSuccess, OnMessage};
use je_workbench::services::search::raw_search_service::{EngineClass, IProgressCallback, SearchService};
use je_workbench::services::search::ripgrep_disk_path::set_rg_disk_path;
use je_workbench::services::search::search::{
    FileSearchStatsType, IFileQuery, IFolderQuery, IRawFileMatch, IRawSearchService, ISearchEngineStats, ISearchStats, ISerializedFileMatch, ISerializedSearchComplete,
    ISerializedSearchEvent, ISerializedSearchProgressItem,
};

fn normalize(path: &str) -> String {
    if cfg!(windows) { path.replace('/', "\\") } else { path.to_owned() }
}

fn test_fixtures() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("search").to_string_lossy().into_owned()
}

/// O `rg` do oráculo, se o repositório o tiver; senão vale o `rg` do `PATH`.
fn use_oracle_ripgrep() {
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ref/oracle/VSCode-linux-x64/resources/app/node_modules.asar.unpacked/@vscode/ripgrep-universal/bin/linux-x64/rg");
    if oracle.is_file() {
        set_rg_disk_path(oracle.to_string_lossy().into_owned());
    }
}

fn test_folder_queries() -> Vec<IFolderQuery> {
    vec![IFolderQuery::new(uri_file(&normalize("/some/where")))]
}

fn multiroot_queries() -> Vec<IFolderQuery> {
    let fixtures = test_fixtures();
    vec![IFolderQuery::new(uri_file(&format!("{fixtures}/examples"))), IFolderQuery::new(uri_file(&format!("{fixtures}/more")))]
}

fn txt_and_js_include_pattern() -> Expression {
    let mut include_pattern = Expression::new();
    include_pattern.insert("*.txt".to_owned(), ExprValue::Bool(true));
    include_pattern.insert("*.js".to_owned(), ExprValue::Bool(true));
    include_pattern
}

fn stats() -> ISearchEngineStats {
    ISearchEngineStats { file_walk_time: 0.0, cmd_time: 1.0, directories_walked: 2.0, files_walked: 3.0, cmd_result_count: None }
}

thread_local! {
    /// `TestSearchEngine.last.config`.
    static LAST_CONFIG: RefCell<Option<IFileQuery>> = const { RefCell::new(None) };
}

struct TestEngineState {
    result: Rc<RefCell<dyn FnMut() -> Option<IRawFileMatch>>>,
    is_canceled: Cell<bool>,
}

/// `TestSearchEngine`.
struct TestSearchEngine {
    state: Rc<TestEngineState>,
}

fn next(state: Rc<TestEngineState>, on_result: Rc<dyn Fn(&IRawFileMatch)>, done: Box<dyn FnOnce(Option<Error>, ISearchEngineSuccess)>) {
    runtime::queue_microtask(move || {
        let success = || ISearchEngineSuccess { limit_hit: false, stats: stats(), messages: Vec::new() };
        if state.is_canceled.get() {
            done(None, success());
            return;
        }
        let result = {
            let mut source = state.result.borrow_mut();
            (&mut *source)()
        };
        match result {
            None => done(None, success()),
            Some(raw_match) => {
                on_result(&raw_match);
                next(state, on_result, done);
            }
        }
    });
}

impl ISearchEngine<IRawFileMatch> for TestSearchEngine {
    fn search(&self, on_result: Rc<dyn Fn(&IRawFileMatch)>, _on_progress: OnMessage, done: Box<dyn FnOnce(Option<Error>, ISearchEngineSuccess)>) {
        next(self.state.clone(), on_result, done);
    }

    fn cancel(&self) {
        self.state.is_canceled.set(true);
    }
}

/// `TestSearchEngine.bind(null, result)`.
fn test_engine(result: impl FnMut() -> Option<IRawFileMatch> + 'static) -> EngineClass {
    let result: Rc<RefCell<dyn FnMut() -> Option<IRawFileMatch>>> = Rc::new(RefCell::new(result));
    Rc::new(move |config: &IFileQuery, _threads: Option<i64>| {
        LAST_CONFIG.with(|last| *last.borrow_mut() = Some(config.clone()));
        Rc::new(TestSearchEngine { state: Rc::new(TestEngineState { result: result.clone(), is_canceled: Cell::new(false) }) }) as Rc<dyn ISearchEngine<IRawFileMatch>>
    })
}

fn raw_search() -> IFileQuery {
    let mut raw_search = IFileQuery::new(test_folder_queries());
    raw_search.file_pattern = Some("a".to_owned());
    raw_search
}

fn raw_match() -> IRawFileMatch {
    IRawFileMatch { base: Some(normalize("/some")), relative_path: "where".to_owned(), search_path: None }
}

fn match_() -> ISerializedFileMatch {
    ISerializedFileMatch::new(normalize("/some/where"))
}

/// Os caminhos de um `IRawFileMatch` sem `searchPath` sob `/some/where`.
fn raw_match_in(relative_path: &str) -> IRawFileMatch {
    IRawFileMatch { base: Some(normalize("/some/where")), relative_path: relative_path.to_owned(), search_path: None }
}

fn new_service() -> Rc<SearchService> {
    SearchService::new(FileSearchStatsType::SearchProcess, None)
}

/// `if (!!(<IProgressMessage>value).message) return;`
fn is_message(value: &ISerializedSearchProgressItem) -> bool {
    matches!(value, ISerializedSearchProgressItem::Message(message) if !message.message.is_empty())
}

/// O callback que junta o caminho dos lotes (`results.push(...value.map(v => v.path))`).
fn collect_paths_callback(results: Rc<RefCell<Vec<String>>>) -> IProgressCallback {
    Rc::new(move |value: ISerializedSearchProgressItem| {
        if is_message(&value) {
            return;
        }
        match value {
            ISerializedSearchProgressItem::FileMatches(matches) => results.borrow_mut().extend(matches.into_iter().map(|m| m.path)),
            other => panic!("{other:?}"),
        }
    })
}

#[test]
fn individual_results() {
    run_with_real_timers(async {
        let mut i = 5;
        let engine = test_engine(move || {
            let current = i;
            i -= 1;
            (current != 0).then(raw_match)
        });
        let service = new_service();

        let results = Rc::new(Cell::new(0));
        let cb: IProgressCallback = {
            let results = results.clone();
            Rc::new(move |value: ISerializedSearchProgressItem| {
                if is_message(&value) {
                    return;
                }
                match value {
                    ISerializedSearchProgressItem::FileMatch(value) => {
                        assert_eq!(value, match_());
                        results.set(results.get() + 1);
                    }
                    other => panic!("{other:?}"),
                }
            })
        };

        service.do_file_search_with_engine(&engine, &raw_search(), cb, None, 0, None).await.unwrap();
        assert_eq!(results.get(), 5);
    });
}

#[test]
fn batch_results() {
    run_with_real_timers(async {
        let mut i = 25;
        let engine = test_engine(move || {
            let current = i;
            i -= 1;
            (current != 0).then(raw_match)
        });
        let service = new_service();

        let results: Rc<RefCell<Vec<usize>>> = Rc::default();
        let cb: IProgressCallback = {
            let results = results.clone();
            Rc::new(move |value: ISerializedSearchProgressItem| {
                if is_message(&value) {
                    return;
                }
                match value {
                    ISerializedSearchProgressItem::FileMatches(value) => {
                        for m in &value {
                            assert_eq!(*m, match_());
                        }
                        results.borrow_mut().push(value.len());
                    }
                    other => panic!("{other:?}"),
                }
            })
        };

        service.do_file_search_with_engine(&engine, &raw_search(), cb, None, 10, None).await.unwrap();
        assert_eq!(*results.borrow(), vec![10, 10, 5]);
    });
}

/// `collectResultsFromEvent`.
#[derive(Clone)]
struct CollectedResults {
    files: Vec<ISerializedFileMatch>,
    limit_hit: bool,
}

fn collect_results_from_event(event: Event<ISerializedSearchEvent>) -> Promise<CollectedResults> {
    let (promise, resolver) = Promise::new();
    let files: Rc<RefCell<Vec<ISerializedFileMatch>>> = Rc::default();
    let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();

    let subscription = {
        let (files, listener) = (files.clone(), listener.clone());
        event.listen(move |ev: &ISerializedSearchEvent| {
            let dispose_listener = || {
                if let Some(listener) = listener.borrow_mut().take() {
                    listener.dispose();
                }
            };
            match ev {
                ISerializedSearchEvent::Complete(ISerializedSearchComplete::Success(success)) => {
                    resolver.resolve(CollectedResults { files: files.borrow().clone(), limit_hit: success.limit_hit });
                    dispose_listener();
                }
                ISerializedSearchEvent::Complete(ISerializedSearchComplete::Error(error)) => {
                    resolver.reject(Error::new(error.message.clone()));
                    dispose_listener();
                }
                ISerializedSearchEvent::Progress(ISerializedSearchProgressItem::FileMatches(matches)) => files.borrow_mut().extend(matches.iter().cloned()),
                ISerializedSearchEvent::Progress(ISerializedSearchProgressItem::FileMatch(file_match)) if !file_match.path.is_empty() => {
                    files.borrow_mut().push(file_match.clone());
                }
                ISerializedSearchEvent::Progress(_) => {}
            }
        })
    };
    *listener.borrow_mut() = Some(subscription);
    promise
}

use je_base::lifecycle::IDisposable;

#[test]
fn collect_batched_results() {
    run_with_real_timers(async {
        let uri_path = "/some/where";
        let mut i = 25;
        let engine = test_engine(move || {
            let current = i;
            i -= 1;
            (current != 0).then(raw_match)
        });
        let service = new_service();

        let file_search = |config: IFileQuery, batch_size: i64| -> Event<ISerializedSearchEvent> {
            let slot: Rc<RefCell<Weak<Emitter<ISerializedSearchEvent>>>> = Rc::default();
            let running: Rc<RefCell<Option<CancelablePromise<()>>>> = Rc::default();

            let on_will_add_first_listener = {
                let (slot, running, service, engine) = (slot.clone(), running.clone(), service.clone(), engine.clone());
                move |_: &Emitter<ISerializedSearchEvent>| {
                    let emitter = slot.borrow().upgrade().expect("o emitter está vivo");
                    let promise = create_cancelable_promise(|token| {
                        let progress_emitter = emitter.clone();
                        let on_progress: IProgressCallback = Rc::new(move |p| progress_emitter.fire(ISerializedSearchEvent::Progress(p)));
                        service.do_file_search_with_engine(&engine, &config, on_progress, Some(&token), batch_size, None).then_result(move |result| {
                            emitter.fire(ISerializedSearchEvent::Complete(match result {
                                Ok(complete) => ISerializedSearchComplete::Success(complete),
                                Err(error) => ISerializedSearchComplete::Error(je_workbench::services::search::search::ISerializedSearchError { message: error.message, stack: error.stack }),
                            }));
                            Ok(())
                        })
                    });
                    *running.borrow_mut() = Some(promise);
                }
            };
            let on_did_remove_last_listener = move |_: &Emitter<ISerializedSearchEvent>| {
                if let Some(promise) = running.borrow().as_ref() {
                    promise.cancel();
                }
            };

            let emitter = Emitter::with_options(EmitterOptions {
                on_will_add_first_listener: Some(Rc::new(on_will_add_first_listener)),
                on_did_remove_last_listener: Some(Rc::new(on_did_remove_last_listener)),
                ..EmitterOptions::default()
            });
            *slot.borrow_mut() = Rc::downgrade(&emitter);
            emitter.event()
        };

        let result = collect_results_from_event(file_search(raw_search(), 10)).await.unwrap();
        for f in &result.files {
            assert_eq!(f.path.replace('\\', "/"), uri_path);
        }
        assert_eq!(result.files.len(), 25, "Result");
    });
}

#[test]
fn multi_root_with_include_pattern_and_max_results() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let service = new_service();

        let mut query = IFileQuery::new(multiroot_queries());
        query.common.max_results = Some(1);
        query.common.include_pattern = Some(txt_and_js_include_pattern());

        let result = collect_results_from_event(service.file_search(query)).await.unwrap();
        assert_eq!(result.files.len(), 1, "Result");
    });
}

#[test]
fn handles_max_results_0_correctly() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let service = new_service();

        let mut query = IFileQuery::new(multiroot_queries());
        query.common.max_results = Some(0);
        query.sort_by_score = Some(true);
        query.common.include_pattern = Some(txt_and_js_include_pattern());

        let result = collect_results_from_event(service.file_search(query)).await.unwrap();
        assert_eq!(result.files.len(), 0, "Result");
    });
}

#[test]
fn multi_root_with_include_pattern_and_exists() {
    use_oracle_ripgrep();
    run_with_real_timers(async {
        let service = new_service();

        let mut query = IFileQuery::new(multiroot_queries());
        query.exists = Some(true);
        query.common.include_pattern = Some(txt_and_js_include_pattern());

        let result = collect_results_from_event(service.file_search(query)).await.unwrap();
        assert_eq!(result.files.len(), 0, "Result");
        assert!(result.limit_hit);
    });
}

#[test]
fn sorted_results() {
    run_with_real_timers(async {
        let matches: Rc<RefCell<VecDeque<IRawFileMatch>>> = Rc::new(RefCell::new(["bab", "bbc", "abb"].into_iter().map(raw_match_in).collect()));
        let engine = test_engine({
            let matches = matches.clone();
            move || matches.borrow_mut().pop_front()
        });
        let service = new_service();

        let results: Rc<RefCell<Vec<String>>> = Rc::default();
        let cb = collect_paths_callback(results.clone());

        let mut query = IFileQuery::new(test_folder_queries());
        query.file_pattern = Some("bb".to_owned());
        query.sort_by_score = Some(true);
        query.common.max_results = Some(2);
        service.do_file_search_with_engine(&engine, &query, cb, None, 1, None).await.unwrap();
        // `typeof TestSearchEngine.last.config.maxResults` não é `'number'`
        assert_eq!(LAST_CONFIG.with(|last| last.borrow().as_ref().expect("o motor foi criado").common.max_results), None);
        assert_eq!(*results.borrow(), vec![normalize("/some/where/bbc"), normalize("/some/where/bab")]);
    });
}

#[test]
fn sorted_result_batches() {
    run_with_real_timers(async {
        let mut i = 25;
        let engine = test_engine(move || {
            let current = i;
            i -= 1;
            (current != 0).then(raw_match)
        });
        let service = new_service();

        let results: Rc<RefCell<Vec<usize>>> = Rc::default();
        let cb: IProgressCallback = {
            let results = results.clone();
            Rc::new(move |value: ISerializedSearchProgressItem| {
                if is_message(&value) {
                    return;
                }
                match value {
                    ISerializedSearchProgressItem::FileMatches(value) => {
                        for m in &value {
                            assert_eq!(*m, match_());
                        }
                        results.borrow_mut().push(value.len());
                    }
                    other => panic!("{other:?}"),
                }
            })
        };
        let mut query = IFileQuery::new(test_folder_queries());
        query.file_pattern = Some("a".to_owned());
        query.sort_by_score = Some(true);
        query.common.max_results = Some(23);
        service.do_file_search_with_engine(&engine, &query, cb, None, 10, None).await.unwrap();
        assert_eq!(*results.borrow(), vec![10, 10, 3]);
    });
}

fn from_cache(stats: &Option<ISearchStats>) -> bool {
    match stats {
        Some(ISearchStats::File(stats)) => stats.from_cache,
        other => panic!("estatísticas de arquivo esperadas: {other:?}"),
    }
}

#[test]
fn cached_results() {
    run_with_real_timers(async {
        let matches: Rc<RefCell<VecDeque<IRawFileMatch>>> = Rc::new(RefCell::new(["bcb", "bbc", "aab"].into_iter().map(raw_match_in).collect()));
        let engine = test_engine({
            let matches = matches.clone();
            move || matches.borrow_mut().pop_front()
        });
        let service = new_service();

        let cached_query = |file_pattern: &str| {
            let mut query = IFileQuery::new(test_folder_queries());
            query.file_pattern = Some(file_pattern.to_owned());
            query.sort_by_score = Some(true);
            query.cache_key = Some("x".to_owned());
            query
        };

        let results: Rc<RefCell<Vec<String>>> = Rc::default();
        let complete = service.do_file_search_with_engine(&engine, &cached_query("b"), collect_paths_callback(results.clone()), None, -1, None).await.unwrap();
        assert!(!from_cache(&complete.stats));
        assert_eq!(*results.borrow(), vec![normalize("/some/where/bcb"), normalize("/some/where/bbc"), normalize("/some/where/aab")]);

        let results: Rc<RefCell<Vec<String>>> = Rc::default();
        // O `try { ... } catch (e) { }` do original engole qualquer falha desta segunda busca.
        if let Ok(complete) = service.do_file_search_with_engine(&engine, &cached_query("bc"), collect_paths_callback(results.clone()), None, -1, None).await {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                assert!(from_cache(&complete.stats));
                assert_eq!(*results.borrow(), vec![normalize("/some/where/bcb"), normalize("/some/where/bbc")]);
            }));
        }

        service.clear_cache("x").await.unwrap();

        matches.borrow_mut().push_back(raw_match_in("bc"));
        let results: Rc<RefCell<Vec<String>>> = Rc::default();
        let complete = service.do_file_search_with_engine(&engine, &cached_query("bc"), collect_paths_callback(results.clone()), None, -1, None).await.unwrap();
        assert!(!from_cache(&complete.stats));
        assert_eq!(*results.borrow(), vec![normalize("/some/where/bc")]);
    });
}
