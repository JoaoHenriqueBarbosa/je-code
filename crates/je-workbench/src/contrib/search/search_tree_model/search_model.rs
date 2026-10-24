//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/searchModel.ts`: o modelo de uma busca
//! (`SearchModelImpl`), que dispara a busca de texto e a de IA, enfileira os resultados e os entrega ao `ISearchResult`,
//! e o `SearchViewModelWorkbenchService`.
//!
//! Divergências:
//! - `ISearchConfigurationProperties` é o [`Dynamic`] da seção `search` (só `searchOnType` é lido).
//! - A telemetria usa `publicLog2` (o `publicLog` do original, com os mesmos eventos e dados como [`Dynamic`]).
//! - `Promise.race([asyncResults, progressEmitterPromise])` converte o resultado da busca em `()` (uma busca rejeitada não
//!   rejeita a promessa que só serve para medir a primeira renderização).
//! - `setTimeout(resolve, delay)` do `_startStreamDelay` é `runtime::set_timeout`.
//! - `SearchViewModelWorkbenchService.searchModel` guarda o modelo como `Rc<dyn ISearchModel>` (o original tipa
//!   `SearchModelImpl`), para o setter do contrato aceitar qualquer modelo.
//! - `async getAITextResultProviderName` e `getAsyncResults` são encadeamentos de `Promise`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, EmitterOptions, Event, PauseableEmitter};
use je_base::lazy::Lazy;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::LogServiceRef;
use je_platform::telemetry::telemetry::ITelemetryService;

use super::search_result::SearchResultImpl;
use super::search_tree_common::{
    IChangeEvent, ISearchModel, ISearchResult, SEARCH_MODEL_PREFIX, SearchModelLocation, SearchModelSearchResults, merge_search_result_events,
};
use super::search_view_model_workbench_service::ISearchViewModelWorkbenchService;
use crate::contrib::search::common::notebook_search::INotebookSearchService;
use crate::services::search::replace::ReplacePattern;
use crate::services::search::search::{
    IAITextQuery, IFileMatch, ISearchComplete, ISearchProgressItem, ISearchService, ISearchStats, ITextQuery, PatternInfo, SearchCompletionExitCode,
    SearchProgressCallback,
};

/// `options` de `searchResultsShown`: o `IPatternInfo` sem o `pattern` (`Object.assign({}, contentPattern)` seguido de
/// `delete options.pattern`).
fn pattern_info_options(content_pattern: &PatternInfo) -> Dynamic {
    let mut options: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(value) = content_pattern.is_reg_exp {
        options.push(("isRegExp", Dynamic::Bool(value)));
    }
    if let Some(value) = content_pattern.is_word_match {
        options.push(("isWordMatch", Dynamic::Bool(value)));
    }
    if let Some(value) = &content_pattern.word_separators {
        options.push(("wordSeparators", Dynamic::string(&value.to_string_lossy())));
    }
    if let Some(value) = content_pattern.is_multiline {
        options.push(("isMultiline", Dynamic::Bool(value)));
    }
    if let Some(value) = content_pattern.is_unicode {
        options.push(("isUnicode", Dynamic::Bool(value)));
    }
    if let Some(value) = content_pattern.is_case_sensitive {
        options.push(("isCaseSensitive", Dynamic::Bool(value)));
    }
    if let Some(info) = content_pattern.notebook_info {
        let mut notebook_info: Vec<(&str, Dynamic)> = Vec::new();
        if let Some(value) = info.is_in_notebook_markdown_input {
            notebook_info.push(("isInNotebookMarkdownInput", Dynamic::Bool(value)));
        }
        if let Some(value) = info.is_in_notebook_markdown_preview {
            notebook_info.push(("isInNotebookMarkdownPreview", Dynamic::Bool(value)));
        }
        if let Some(value) = info.is_in_notebook_cell_input {
            notebook_info.push(("isInNotebookCellInput", Dynamic::Bool(value)));
        }
        if let Some(value) = info.is_in_notebook_cell_output {
            notebook_info.push(("isInNotebookCellOutput", Dynamic::Bool(value)));
        }
        options.push(("notebookInfo", Dynamic::object(notebook_info)));
    }
    Dynamic::object(options)
}

/// `{ duration }` dos eventos de telemetria de duração.
fn duration_data(duration: f64) -> Option<Dynamic> {
    Some(Dynamic::object([("duration", Dynamic::Number(duration))]))
}

/// `aiTextQueryFromTextQuery`, para `{ ...this._searchQuery, contentPattern: this._searchQuery.contentPattern.pattern, type: QueryType.aiText }`.
fn ai_text_query(query: &ITextQuery) -> IAITextQuery {
    IAITextQuery {
        common: query.common.clone(),
        content_pattern: query.content_pattern.pattern.clone(),
        preview_options: query.preview_options,
        max_file_size: query.max_file_size,
        surrounding_context: query.surrounding_context,
        user_disabled_excludes_and_ignore_files: query.user_disabled_excludes_and_ignore_files,
    }
}

/// `SearchModelImpl`.
pub struct SearchModelImpl {
    this: Weak<SearchModelImpl>,
    base: Rc<Disposable>,
    search_result: Rc<dyn ISearchResult>,
    search_query: RefCell<Option<ITextQuery>>,
    replace_active: Cell<bool>,
    replace_string: RefCell<Option<String>>,
    replace_pattern: RefCell<Option<Rc<ReplacePattern>>>,
    preserve_case: Cell<bool>,
    start_stream_delay: RefCell<Promise<()>>,
    result_queue: RefCell<Vec<IFileMatch>>,
    ai_result_queue: RefCell<Vec<IFileMatch>>,

    on_replace_term_changed: Rc<Emitter<()>>,
    on_search_result_changed: Rc<PauseableEmitter<IChangeEvent>>,

    current_cancel_token_source: RefCell<Option<Rc<CancellationTokenSource>>>,
    current_ai_cancel_token_source: RefCell<Option<Rc<CancellationTokenSource>>>,
    search_cancelled_for_new_search: Cell<bool>,
    ai_search_cancelled_for_new_search: Cell<bool>,
    location: Cell<SearchModelLocation>,
    ai_text_result_provider_name: Lazy<Promise<Option<String>>>,

    id: String,

    search_service: Rc<dyn ISearchService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: LogServiceRef,
    notebook_search_service: Rc<dyn INotebookSearchService>,
}

impl Injectable for SearchModelImpl {
    type Args = ();
    type Deps = (
        Rc<dyn ISearchService>,
        Rc<dyn ITelemetryService>,
        Rc<dyn IConfigurationService>,
        Rc<InstantiationService>,
        LogServiceRef,
        Rc<dyn INotebookSearchService>,
    );

    fn create(
        _: (),
        (search_service, telemetry_service, configuration_service, instantiation_service, log_service, notebook_search_service): Self::Deps,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<SearchModelImpl>| {
            let base = Disposable::new();
            let on_replace_term_changed = Emitter::<()>::new();
            base.register(on_replace_term_changed.clone());
            let on_search_result_changed =
                PauseableEmitter::<IChangeEvent>::with_options(EmitterOptions::default(), Some(Rc::new(merge_search_result_events)));
            base.register(on_search_result_changed.clone());

            let search_model: Weak<dyn ISearchModel> = this.clone();
            let search_result: Rc<dyn ISearchResult> = instantiation_service.create_instance::<SearchResultImpl>(search_model);
            let emitter = on_search_result_changed.clone();
            base.register(search_result.on_change().listen(move |event| emitter.fire(event.clone())));

            let provider_search_service = search_service.clone();
            Self {
                this: this.clone(),
                base,
                search_result,
                search_query: RefCell::new(None),
                replace_active: Cell::new(false),
                replace_string: RefCell::new(None),
                replace_pattern: RefCell::new(None),
                preserve_case: Cell::new(false),
                start_stream_delay: RefCell::new(Promise::resolved(())),
                result_queue: RefCell::new(Vec::new()),
                ai_result_queue: RefCell::new(Vec::new()),
                on_replace_term_changed,
                on_search_result_changed,
                current_cancel_token_source: RefCell::new(None),
                current_ai_cancel_token_source: RefCell::new(None),
                search_cancelled_for_new_search: Cell::new(false),
                ai_search_cancelled_for_new_search: Cell::new(false),
                location: Cell::new(SearchModelLocation::Panel),
                ai_text_result_provider_name: Lazy::new(move || provider_search_service.get_ai_name()),
                id: format!("{SEARCH_MODEL_PREFIX}{}", runtime::date_now()),
                search_service,
                telemetry_service,
                configuration_service,
                log_service,
                notebook_search_service,
            }
        })
    }
}

impl SearchModelImpl {
    fn do_search(
        &self,
        query: &ITextQuery,
        progress_emitter: &Rc<Emitter<()>>,
        search_instance_id: &str,
        on_progress: Option<SearchProgressCallback>,
        caller_token: Option<CancellationToken>,
    ) -> SearchModelSearchResults {
        let make_on_progress = |sync: bool| -> SearchProgressCallback {
            let progress_emitter = progress_emitter.clone();
            let this = self.this.clone();
            let search_instance_id = search_instance_id.to_string();
            let on_progress = on_progress.clone();
            Rc::new(move |p: &ISearchProgressItem| {
                progress_emitter.fire(());
                if let Some(this) = this.upgrade() {
                    this.on_search_progress(p, &search_instance_id, sync, false);
                }
                if let Some(on_progress) = &on_progress {
                    on_progress(p);
                }
            })
        };
        let async_generate_on_progress = make_on_progress(false);
        let sync_generate_on_progress = make_on_progress(true);

        let token_source = Rc::new(CancellationTokenSource::new(caller_token.as_ref()));
        *self.current_cancel_token_source.borrow_mut() = Some(token_source.clone());

        let notebook_result =
            self.notebook_search_service.notebook_search(query, Some(token_source.token()), search_instance_id, Some(async_generate_on_progress.clone()));
        let text_result = self.search_service.text_search_split_sync_async(
            query,
            Some(token_source.token()),
            Some(async_generate_on_progress),
            Some(notebook_result.open_files_to_scan.clone()),
            Some(notebook_result.all_scanned_files.clone()),
        );

        let sync_results = text_result.sync_results.results.clone();
        for p in &sync_results {
            sync_generate_on_progress(&ISearchProgressItem::FileMatch(p.clone()));
        }

        let log_service = self.log_service.clone();
        let complete_data = notebook_result.complete_data.clone();
        let search_start = runtime::date_now();
        let async_results = text_result.async_results.then_promise(move |all_closed_editor_results| {
            // resolve async parts of search
            complete_data.then(move |resolved_notebook_results| {
                let search_length = runtime::date_now() - search_start;
                let resolved_result = ISearchComplete {
                    results: all_closed_editor_results.results.iter().chain(resolved_notebook_results.results.iter()).cloned().collect(),
                    messages: all_closed_editor_results.messages.iter().chain(resolved_notebook_results.messages.iter()).cloned().collect(),
                    limit_hit: if all_closed_editor_results.limit_hit == Some(true) {
                        all_closed_editor_results.limit_hit
                    } else {
                        resolved_notebook_results.limit_hit
                    },
                    exit: all_closed_editor_results.exit,
                    stats: all_closed_editor_results.stats,
                    ai_keywords: None,
                };
                log_service.trace(&format!("whole search time | {search_length}ms"));
                resolved_result
            })
        });
        SearchModelSearchResults { async_results: async_results.finally(move || token_source.dispose_with(true)), sync_results }
    }

    fn search_on_type(&self) -> bool {
        let search_config = self.configuration_service.get_value(Some("search"), None);
        search_config.is_truthy() && search_config.get_str("searchOnType").is_truthy()
    }

    fn on_search_completed(&self, completed: Option<&ISearchComplete>, duration: f64, search_instance_id: &str, ai: bool) -> Option<ISearchComplete> {
        let Some(search_query) = self.search_query.borrow().clone() else {
            errors::throw(Error::new("onSearchCompleted must be called after a search is started"))
        };

        let queue = if ai { &self.ai_result_queue } else { &self.result_queue };
        let queued = std::mem::take(&mut *queue.borrow_mut());
        self.search_result.add(&queued, search_instance_id, ai, false);

        self.search_result.set_cached_search_complete(completed.cloned(), ai);

        let options = pattern_info_options(&search_query.content_pattern);

        let stats_type = completed.and_then(|completed| match completed.stats {
            Some(ISearchStats::Text(stats)) => Some(stats.stats_type.as_str()),
            _ => None,
        });

        let folder_queries = &search_query.common.folder_queries;
        let file_scheme_only = folder_queries.iter().all(|folder_query| folder_query.folder.scheme().to_string_lossy() == "file");
        let other_scheme_only = folder_queries.iter().all(|folder_query| folder_query.folder.scheme().to_string_lossy() != "file");
        let scheme = if file_scheme_only {
            "file"
        } else if other_scheme_only {
            "other"
        } else {
            "mixed"
        };

        self.telemetry_service.public_log2(
            "searchResultsShown",
            Some(Dynamic::object([
                ("count", Dynamic::Number(self.search_result.count(false) as f64)),
                ("fileCount", Dynamic::Number(self.search_result.file_count(false) as f64)),
                ("options", options),
                ("duration", Dynamic::Number(duration)),
                ("type", stats_type.map_or(Dynamic::Undefined, Dynamic::string)),
                ("scheme", Dynamic::string(scheme)),
                ("searchOnTypeEnabled", Dynamic::Bool(self.search_on_type())),
            ])),
        );
        completed.cloned()
    }

    fn on_search_error(&self, e: &Error, duration: f64, ai: bool) {
        if errors::is_cancellation_error(e) {
            let cancelled_for_new_search = if ai { &self.ai_search_cancelled_for_new_search } else { &self.search_cancelled_for_new_search };
            let completed = cancelled_for_new_search.get().then(|| ISearchComplete {
                limit_hit: None,
                messages: Vec::new(),
                stats: None,
                results: Vec::new(),
                exit: Some(SearchCompletionExitCode::NewSearchStarted),
                ai_keywords: None,
            });
            self.on_search_completed(completed.as_ref(), duration, "", ai);
            cancelled_for_new_search.set(false);
        }
    }

    fn on_search_progress(&self, p: &ISearchProgressItem, search_instance_id: &str, sync: bool, ai: bool) {
        let Some(file_match) = p.as_file_match() else {
            return;
        };
        let target_queue = if ai { &self.ai_result_queue } else { &self.result_queue };
        target_queue.borrow_mut().push(file_match.clone());
        if sync {
            if !target_queue.borrow().is_empty() {
                let queued = std::mem::take(&mut *target_queue.borrow_mut());
                self.search_result.add(&queued, search_instance_id, false, true);
            }
        } else {
            let this = self.this.clone();
            let search_instance_id = search_instance_id.to_string();
            let _ = self.start_stream_delay.borrow().clone().then(move |_| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                let target_queue = if ai { &this.ai_result_queue } else { &this.result_queue };
                if !target_queue.borrow().is_empty() {
                    let queued = std::mem::take(&mut *target_queue.borrow_mut());
                    this.search_result.add(&queued, &search_instance_id, ai, !ai);
                }
            });
        }
    }

    fn make_replace_pattern(&self, replace_string: &str, content_pattern: &PatternInfo) -> Rc<ReplacePattern> {
        match ReplacePattern::from_pattern_info(replace_string, content_pattern) {
            Ok(replace_pattern) => Rc::new(replace_pattern),
            Err(error) => errors::throw(Error::new(format!("{error:?}"))),
        }
    }
}

impl IDisposable for SearchModelImpl {
    fn dispose(&self) {
        self.cancel_search(false);
        self.cancel_ai_search(false);
        self.search_result.dispose();
        self.base.dispose();
    }
}

impl ISearchModel for SearchModelImpl {
    fn on_replace_term_changed(&self) -> Event<()> {
        self.on_replace_term_changed.event()
    }

    fn on_search_result_changed(&self) -> Event<IChangeEvent> {
        self.on_search_result_changed.event()
    }

    fn location(&self) -> SearchModelLocation {
        self.location.get()
    }

    fn set_location(&self, location: SearchModelLocation) {
        self.location.set(location);
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn get_ai_text_result_provider_name(&self) -> Promise<String> {
        self.ai_text_result_provider_name.value().then_result(|result| match result {
            Ok(Some(name)) => Ok(name),
            Ok(None) => Err(Error::new("Fetching AI name when no provider present.")),
            Err(error) => Err(error),
        })
    }

    fn is_replace_active(&self) -> bool {
        self.replace_active.get()
    }

    fn set_replace_active(&self, replace_active: bool) {
        self.replace_active.set(replace_active);
    }

    fn replace_pattern(&self) -> Option<Rc<ReplacePattern>> {
        self.replace_pattern.borrow().clone()
    }

    fn replace_string(&self) -> String {
        self.replace_string.borrow().clone().unwrap_or_default()
    }

    fn set_replace_string(&self, replace_string: &str) {
        *self.replace_string.borrow_mut() = Some(replace_string.to_string());
        let search_query = self.search_query.borrow().clone();
        if let Some(search_query) = search_query {
            *self.replace_pattern.borrow_mut() = Some(self.make_replace_pattern(replace_string, &search_query.content_pattern));
        }
        self.on_replace_term_changed.fire(());
    }

    fn preserve_case(&self) -> bool {
        self.preserve_case.get()
    }

    fn set_preserve_case(&self, value: bool) {
        self.preserve_case.set(value);
    }

    fn search_result(&self) -> Rc<dyn ISearchResult> {
        self.search_result.clone()
    }

    fn ai_search(&self, on_result: Rc<dyn Fn(Option<&ISearchProgressItem>)>) -> Promise<ISearchComplete> {
        if self.has_ai_results() {
            // already has matches or pending matches
            errors::throw(Error::new("AI results already exist"));
        }
        let Some(search_query) = self.search_query.borrow().clone() else {
            errors::throw(Error::new("No search query"))
        };

        let search_instance_id = runtime::date_now().to_string();
        let token_source = Rc::new(CancellationTokenSource::new(None));
        *self.current_ai_cancel_token_source.borrow_mut() = Some(token_source.clone());
        let start = runtime::date_now();

        let progress_target = self.this.clone();
        let progress_search_instance_id = search_instance_id.clone();
        let progress_result = on_result.clone();
        let on_progress: SearchProgressCallback = Rc::new(move |p: &ISearchProgressItem| {
            progress_result(Some(p));
            if let Some(this) = progress_target.upgrade() {
                this.on_search_progress(p, &progress_search_instance_id, false, true);
            }
        });
        let async_ai_results = self.search_service.ai_text_search(&ai_text_query(&search_query), Some(token_source.token()), Some(on_progress));
        let this = self.this.clone();
        async_ai_results.finally(move || token_source.dispose_with(true)).then_result(move |result| {
            let duration = (runtime::date_now() - start) as f64;
            let Some(this) = this.upgrade() else {
                return result;
            };
            match result {
                Ok(value) => {
                    if value.results.is_empty() {
                        // alert of no results since onProgress won't be called
                        on_result(None);
                    }
                    this.on_search_completed(Some(&value), duration, &search_instance_id, true);
                    Ok(value)
                }
                Err(error) => {
                    this.on_search_error(&error, duration, true);
                    Err(error)
                }
            }
        })
    }

    fn has_ai_results(&self) -> bool {
        self.search_result.get_cached_search_complete(true).is_some()
            || self.current_ai_cancel_token_source.borrow().as_ref().is_some_and(|source| !source.token().is_cancellation_requested())
    }

    fn has_plain_results(&self) -> bool {
        self.search_result.get_cached_search_complete(false).is_some()
            || self.current_cancel_token_source.borrow().as_ref().is_some_and(|source| !source.token().is_cancellation_requested())
    }

    fn search(&self, query: ITextQuery, on_progress: Option<SearchProgressCallback>, caller_token: Option<CancellationToken>) -> SearchModelSearchResults {
        self.cancel_search(true);

        *self.search_query.borrow_mut() = Some(query.clone());
        let search_on_type = self.search_on_type();
        if !search_on_type {
            self.search_result.clear();
        }
        let search_instance_id = runtime::date_now().to_string();

        self.search_result.set_query(Some(query.clone()));

        let progress_emitter = Emitter::<()>::new();
        self.base.register(progress_emitter.clone());
        *self.replace_pattern.borrow_mut() = Some(self.make_replace_pattern(&self.replace_string(), &query.content_pattern));

        // In search on type case, delay the streaming of results just a bit, so that we don't flash the only "local results" fast path
        let delay = if search_on_type { 150 } else { 0 };
        *self.start_stream_delay.borrow_mut() = Promise::with_executor(|resolver| {
            runtime::set_timeout(move || resolver.resolve(()), delay);
        });

        let req = self.do_search(&query, &progress_emitter, &search_instance_id, on_progress.clone(), caller_token);
        let async_results = req.async_results;
        let sync_results = req.sync_results;

        if let Some(on_progress) = &on_progress {
            for p in &sync_results {
                on_progress(&ISearchProgressItem::FileMatch(p.clone()));
            }
        }

        let start = runtime::date_now();

        let (progress_emitter_promise, progress_resolver) = Promise::<()>::new();
        let event = progress_emitter.event().once().listen(move |_| progress_resolver.resolve(()));

        let first_render_telemetry = self.telemetry_service.clone();
        let _ = Promise::race(vec![async_results.then_result(|_| Ok(())), progress_emitter_promise]).finally(move || {
            event.dispose();
            first_render_telemetry.public_log2("searchResultsFirstRender", duration_data((runtime::date_now() - start) as f64));
        });

        let completed_target = self.this.clone();
        let error_target = self.this.clone();
        let completed_search_instance_id = search_instance_id;
        let result = SearchModelSearchResults {
            async_results: async_results.then_result(move |result| match result {
                Ok(value) => {
                    if let Some(this) = completed_target.upgrade() {
                        this.on_search_completed(Some(&value), (runtime::date_now() - start) as f64, &completed_search_instance_id, false);
                    }
                    Ok(value)
                }
                Err(error) => {
                    if let Some(this) = error_target.upgrade() {
                        this.on_search_error(&error, (runtime::date_now() - start) as f64, false);
                    }
                    Err(error)
                }
            }),
            sync_results,
        };
        self.telemetry_service.public_log2("searchResultsFinished", duration_data((runtime::date_now() - start) as f64));
        result
    }

    fn cancel_search(&self, cancelled_for_new_search: bool) -> bool {
        let source = self.current_cancel_token_source.borrow().clone();
        if let Some(source) = source {
            self.search_cancelled_for_new_search.set(cancelled_for_new_search);
            source.cancel();
            return true;
        }
        false
    }

    fn cancel_ai_search(&self, cancelled_for_new_search: bool) -> bool {
        let source = self.current_ai_cancel_token_source.borrow().clone();
        if let Some(source) = source {
            self.ai_search_cancelled_for_new_search.set(cancelled_for_new_search);
            source.cancel();
            return true;
        }
        false
    }

    fn clear_ai_search_results(&self) {
        self.ai_result_queue.borrow_mut().clear();
        // it's not clear all as we are only clearing the AI results
        self.search_result.ai_text_search_result().clear(false);
    }
}

/// `SearchViewModelWorkbenchService`.
pub struct SearchViewModelWorkbenchService {
    instantiation_service: Rc<InstantiationService>,
    search_model: RefCell<Option<Rc<dyn ISearchModel>>>,
}

impl Injectable for SearchViewModelWorkbenchService {
    type Args = ();
    type Deps = (Rc<InstantiationService>,);

    fn create(_: (), (instantiation_service,): Self::Deps) -> Rc<Self> {
        Rc::new(Self { instantiation_service, search_model: RefCell::new(None) })
    }
}

impl ISearchViewModelWorkbenchService for SearchViewModelWorkbenchService {
    fn search_model(&self) -> Rc<dyn ISearchModel> {
        let existing = self.search_model.borrow().clone();
        match existing {
            Some(search_model) => search_model,
            None => {
                let search_model: Rc<dyn ISearchModel> = self.instantiation_service.create_instance::<SearchModelImpl>(());
                *self.search_model.borrow_mut() = Some(search_model.clone());
                search_model
            }
        }
    }

    fn set_search_model(&self, search_model: Rc<dyn ISearchModel>) {
        let previous = self.search_model.borrow_mut().replace(search_model);
        if let Some(previous) = previous {
            previous.dispose();
        }
    }
}
