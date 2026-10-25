//! Porte de `src/vs/workbench/services/search/common/fileSearchManager.ts`: o `FileSearchManager`, que roda a busca de
//! arquivos de um `FileSearchProvider2` (e a faz respeitar include, exclude e siblings), e o `FileSearchEngine` e o
//! `SessionLifecycle` internos.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `Promise` é a [`Promise`] do porte; `fileSearch` devolve a promessa do `ISearchCompleteStats`. Erros do provedor
//!   rejeitam com `new Error(toErrorMessage(err))`, como no original.
//! - `rootEntries` e `pathToEntries['.']` do `IDirectoryTree` são o mesmo array no original (`tree.pathToEntries['.'] =
//!   tree.rootEntries`); aqui só existe o mapa, e as entradas da raiz são a chave `.`.
//! - `IInternalFileMatch` não tem `original` nem `size`: o original declara os dois e nunca os lê nem escreve.
//! - `session: unknown` de `FileSearchProviderOptions` é o `Rc<dyn Any>` do objeto da sessão. Sem `cacheKey` o original
//!   passa `undefined`; aqui é um objeto vazio novo, que o provedor não tem como confundir com o de uma sessão.
//! - `this.provider instanceof OldFileSearchProviderConverter ? sessionLifecycle?.tokenSource.token : sessionLifecycle?.obj`
//!   é [`FileSearchProvider2::is_old_file_search_provider_converter`]: o conversor entrega ao provedor antigo o token da
//!   sessão (um [`CancellationToken`] dentro do `Rc<dyn Any>` de `session`, que o conversor devolve a `FileSearchOptions`),
//!   e o `tokenSource` é cancelado por `cancel`.
//! - o `location.folder` que o original lê com `!` (`findQueryFragmentAwareSubstr(result)!`) vira o erro de `TypeError`
//!   que o JavaScript lançaria se a pasta não fosse achada.
//! - `strings.equals(a, b, ignoreCase)` sobre texto opcional é [`strings_equals`], sobre unidades UTF-16.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::error_message::to_error_message_of;
use je_base::errors::{self, Error};
use je_base::glob::{self, GlobOptions, ParsedExpression};
use je_base::lifecycle::IDisposable;
use je_base::path::PathModule;
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;
use je_base::strings;
use je_base::uri::URI;
use je_platform::resources::{host_environment, join_path};

use super::folder_query_search_tree::FolderQuerySearchTree;
use super::search::{
    DEFAULT_MAX_SEARCH_RESULTS, ExcludesForFolder, FileSearchDetailStats, FileSearchStatsType, IFileMatch, IFileQuery, IFileSearchProviderStats, IFileSearchStats, IFolderQuery,
    ISearchCompleteStats, ISearchStats, QueryGlobTester, exclude_to_glob_pattern, expr_match_is_truthy, has_sibling_fn, resolve_patterns_for_provider,
};
use super::search_ext_types::{FileSearchProvider2, FileSearchProviderFolderOptions, FileSearchProviderOptions, UseIgnoreFiles};

/// `IInternalFileMatch`.
#[derive(Clone)]
struct IInternalFileMatch {
    base: URI,
    /// Not present for extraFiles or absolute path matches.
    relative_path: Option<String>,
    basename: String,
}

/// `IDirectoryEntry`.
struct IDirectoryEntry {
    base: URI,
    relative_path: String,
    basename: String,
}

impl IDirectoryEntry {
    /// O `IDirectoryEntry` passado onde o original espera um `IInternalFileMatch` (a forma estrutural é a mesma).
    fn as_match(&self) -> IInternalFileMatch {
        IInternalFileMatch { base: self.base.clone(), relative_path: Some(self.relative_path.clone()), basename: self.basename.clone() }
    }
}

/// `IDirectoryTree`: o `rootEntries` é `path_to_entries["."]`.
struct IDirectoryTree {
    path_to_entries: HashMap<String, Vec<IDirectoryEntry>>,
}

/// `FolderQueryInfo`.
#[derive(Clone)]
struct FolderQueryInfo {
    query_tester: Rc<QueryGlobTester>,
    no_siblings_clauses: bool,
    folder: URI,
    tree: Rc<RefCell<IDirectoryTree>>,
}

/// `IInternalSearchComplete`.
#[derive(Clone)]
struct IInternalSearchComplete {
    limit_hit: bool,
    stats: Option<IFileSearchProviderStats>,
}

/// `path.basename` (a do sistema).
fn path_basename(path: &str) -> String {
    let env = host_environment();
    PathModule::native(env.as_ref()).basename(path, None)
}

/// `path.dirname` (a do sistema).
fn path_dirname(path: &str) -> String {
    let env = host_environment();
    PathModule::native(env.as_ref()).dirname(path)
}

/// `strings.equals(a, b, ignoreCase)`, com `None` no lugar do `undefined`.
fn strings_equals(a: Option<&str>, b: Option<&str>, ignore_case: bool) -> bool {
    let a: Option<Vec<u16>> = a.map(|text| text.encode_utf16().collect());
    let b: Option<Vec<u16>> = b.map(|text| text.encode_utf16().collect());
    strings::equals(a.as_deref(), b.as_deref(), ignore_case)
}

fn glob_options(ignore_glob_case: bool) -> GlobOptions {
    if ignore_glob_case { GlobOptions { ignore_case: Some(true), ..GlobOptions::default() } } else { GlobOptions::default() }
}

/// `initDirectoryTree`.
fn init_directory_tree() -> IDirectoryTree {
    IDirectoryTree { path_to_entries: HashMap::from([(".".to_owned(), Vec::new())]) }
}

type ResultCallback = Rc<dyn Fn(IInternalFileMatch)>;

/// `FileSearchEngine`.
struct FileSearchEngine {
    config: IFileQuery,
    provider: Rc<dyn FileSearchProvider2>,
    session_lifecycle: Option<Rc<SessionLifecycle>>,

    file_pattern: Option<String>,
    include_pattern: Option<ParsedExpression>,
    max_results: Option<i64>,
    exists: Option<bool>,
    is_limit_hit: Cell<bool>,
    result_count: Cell<i64>,
    is_canceled: Cell<bool>,

    active_cancellation_tokens: RefCell<Vec<Rc<CancellationTokenSource>>>,

    global_exclude_pattern: Option<ParsedExpression>,
}

impl FileSearchEngine {
    fn new(config: IFileQuery, provider: Rc<dyn FileSearchProvider2>, session_lifecycle: Option<Rc<SessionLifecycle>>) -> Rc<Self> {
        let options = glob_options(config.common.ignore_glob_case.unwrap_or(false));
        let include_pattern = config.common.include_pattern.as_ref().map(|include| glob::parse_expression(include, &options));
        let global_exclude_pattern = config.common.exclude_pattern.as_ref().map(|exclude| glob::parse_expression(exclude, &options));
        Rc::new(Self {
            file_pattern: config.file_pattern.clone(),
            include_pattern,
            max_results: config.common.max_results.filter(|max_results| *max_results != 0),
            exists: config.exists,
            is_limit_hit: Cell::new(false),
            result_count: Cell::new(0),
            is_canceled: Cell::new(false),
            active_cancellation_tokens: RefCell::new(Vec::new()),
            global_exclude_pattern,
            config,
            provider,
            session_lifecycle,
        })
    }

    fn cancel(&self) {
        self.is_canceled.set(true);
        let tokens = self.active_cancellation_tokens.take();
        for token in tokens {
            token.cancel();
        }
    }

    fn ignore_glob_case(&self) -> bool {
        self.config.common.ignore_glob_case.unwrap_or(false)
    }

    fn search(self: &Rc<Self>, on_result: ResultCallback) -> Promise<IInternalSearchComplete> {
        let folder_queries = self.config.common.folder_queries.clone();

        let engine = self.clone();
        let on_result: ResultCallback = Rc::new(move |match_: IInternalFileMatch| {
            engine.result_count.set(engine.result_count.get() + 1);
            on_result(match_);
        });

        // Support that the file pattern is a full path to a file that exists
        if self.is_canceled.get() {
            return Promise::resolved(IInternalSearchComplete { limit_hit: self.is_limit_hit.get(), stats: None });
        }

        // For each extra file
        if let Some(extra_file_resources) = &self.config.common.extra_file_resources {
            for extra_file in extra_file_resources {
                let extra_file_str = extra_file.to_string(false).to_string_lossy(); // ?
                let basename = path_basename(&extra_file_str);
                if self.global_exclude_pattern.as_ref().is_some_and(|exclude| expr_match_is_truthy(&exclude.matches(&extra_file_str, Some(&basename), None))) {
                    continue; // excluded
                }

                // File: Check for match on file pattern and include pattern
                self.match_file(&on_result, IInternalFileMatch { base: extra_file.clone(), relative_path: None, basename });
            }
        }

        // For each root folder'

        // NEW: can just call with an array of folder info
        let engine = self.clone();
        self.do_search(folder_queries, on_result).then_result(move |stats| match stats {
            Ok(stats) => Ok(IInternalSearchComplete {
                limit_hit: engine.is_limit_hit.get(),
                stats, // Only looking at single-folder workspace stats...
            }),
            Err(err) => Err(Error::new(to_error_message_of(&err, false))),
        })
    }

    fn do_search(self: &Rc<Self>, fqs: Vec<IFolderQuery<URI>>, on_result: ResultCallback) -> Promise<Option<IFileSearchProviderStats>> {
        let cancellation = Rc::new(CancellationTokenSource::new(None));
        let folder_options = fqs.iter().map(|fq| self.get_search_options_for_folder(fq)).collect();
        let session: Rc<dyn Any> = match &self.session_lifecycle {
            Some(session_lifecycle) => match session_lifecycle.session(self.provider.is_old_file_search_provider_converter()) {
                Ok(session) => session,
                Err(error) => {
                    cancellation.dispose();
                    return Promise::rejected(error);
                }
            },
            None => Rc::new(()),
        };
        let options = FileSearchProviderOptions {
            folder_options,
            max_results: self.config.common.max_results.unwrap_or(DEFAULT_MAX_SEARCH_RESULTS as i64),
            session,
        };

        let get_folder_query_info = |fq: &IFolderQuery<URI>, _: usize| {
            let query_tester = Rc::new(QueryGlobTester::new(&self.config.common, fq));
            let no_siblings_clauses = !query_tester.has_sibling_exclude_clauses();
            FolderQueryInfo { query_tester, no_siblings_clauses, folder: fq.folder.clone(), tree: Rc::new(RefCell::new(init_directory_tree())) }
        };

        let folder_mappings: Rc<FolderQuerySearchTree<FolderQueryInfo>> = Rc::new(FolderQuerySearchTree::new(&fqs, get_folder_query_info, None));

        self.active_cancellation_tokens.borrow_mut().push(cancellation.clone());

        let provider_sw = StopWatch::create(None);
        let provided = errors::catch(|| self.provider.provide_file_search_results(self.config.file_pattern.as_deref().unwrap_or(""), &options, &cancellation.token()))
            .unwrap_or_else(Promise::rejected);

        let engine = self.clone();
        provided.then_result(move |results| {
            let outcome = results.and_then(|results| {
                let provider_time = provider_sw.elapsed();
                errors::catch(|| engine.process_results(results, &folder_mappings, &on_result, provider_time)).and_then(|outcome| outcome)
            });
            cancellation.dispose();
            engine.active_cancellation_tokens.borrow_mut().retain(|token| !Rc::ptr_eq(token, &cancellation));
            outcome
        })
    }

    /// O que `doSearch` faz depois do `await` do provedor, até o `return` com as estatísticas.
    fn process_results(
        &self,
        results: Option<Vec<URI>>,
        folder_mappings: &FolderQuerySearchTree<FolderQueryInfo>,
        on_result: &ResultCallback,
        provider_time: f64,
    ) -> Result<Option<IFileSearchProviderStats>, Error> {
        let post_process_sw = StopWatch::create(None);

        if self.is_canceled.get() && !self.is_limit_hit.get() {
            return Ok(None);
        }

        if let Some(results) = results {
            for result in &results {
                let fq_folder_info = folder_mappings
                    .find_query_fragment_aware_substr(result)
                    .ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'folder')"))?;
                let result_path = result.path().to_string_lossy();
                let env = host_environment();
                let relative_path = PathModule::posix(env.as_ref()).relative(&fq_folder_info.folder.path().to_string_lossy(), &result_path);

                if fq_folder_info.no_siblings_clauses {
                    let basename = path_basename(&result_path);
                    self.match_file(on_result, IInternalFileMatch { base: fq_folder_info.folder.clone(), relative_path: Some(relative_path), basename });

                    continue;
                }

                // TODO do original: Optimize siblings clauses with ripgrep here.
                self.add_directory_entries(&fq_folder_info.tree, &fq_folder_info.folder, &relative_path, on_result);
            }
        }

        if self.is_canceled.get() && !self.is_limit_hit.get() {
            return Ok(None);
        }

        folder_mappings.for_each_folder_query_info(|e| {
            self.match_directory_tree(&e.tree.borrow(), &e.query_tester, on_result);
        });

        Ok(Some(IFileSearchProviderStats { provider_time, post_process_time: post_process_sw.elapsed() }))
    }

    fn get_search_options_for_folder(&self, fq: &IFolderQuery<URI>) -> FileSearchProviderFolderOptions {
        let common = &self.config.common;
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

        FileSearchProviderFolderOptions {
            folder: fq.folder.clone(),
            excludes,
            includes,
            use_ignore_files: UseIgnoreFiles {
                local: !fq.disregard_ignore_files.unwrap_or(false),
                parent: !fq.disregard_parent_ignore_files.unwrap_or(false),
                global: !fq.disregard_global_ignore_files.unwrap_or(false),
            },
            follow_symlinks: !fq.ignore_symlinks.unwrap_or(false),
        }
    }

    fn add_directory_entries(&self, tree: &Rc<RefCell<IDirectoryTree>>, base: &URI, relative_file: &str, on_result: &ResultCallback) {
        // Support relative paths to files from a root resource (ignores excludes)
        if let Some(file_pattern) = self.file_pattern.as_deref()
            && !file_pattern.is_empty()
            && strings_equals(Some(relative_file), Some(file_pattern), self.ignore_glob_case())
        {
            let basename = path_basename(file_pattern);
            self.match_file(on_result, IInternalFileMatch { base: base.clone(), relative_path: Some(file_pattern.to_owned()), basename });
        }

        fn add(path_to_entries: &mut HashMap<String, Vec<IDirectoryEntry>>, base: &URI, relative_path: &str) {
            let basename = path_basename(relative_path);
            let dirname = path_dirname(relative_path);
            if !path_to_entries.contains_key(&dirname) {
                path_to_entries.insert(dirname.clone(), Vec::new());
                add(path_to_entries, base, &dirname);
            }
            if let Some(entries) = path_to_entries.get_mut(&dirname) {
                entries.push(IDirectoryEntry { base: base.clone(), relative_path: relative_path.to_owned(), basename });
            }
        }

        add(&mut tree.borrow_mut().path_to_entries, base, relative_file);
    }

    fn match_directory_tree(&self, tree: &IDirectoryTree, query_tester: &QueryGlobTester, on_result: &ResultCallback) {
        let file_pattern = self.file_pattern.as_deref();
        let ignore_glob_case = self.ignore_glob_case();

        fn match_directory(
            engine: &FileSearchEngine,
            path_to_entries: &HashMap<String, Vec<IDirectoryEntry>>,
            entries: &[IDirectoryEntry],
            query_tester: &QueryGlobTester,
            file_pattern: Option<&str>,
            ignore_glob_case: bool,
            on_result: &ResultCallback,
        ) {
            let basenames: Vec<String> = entries.iter().map(|entry| entry.basename.clone()).collect();
            let has_sibling = has_sibling_fn(Some(Rc::new(move || basenames.clone())));
            for entry in entries {
                let relative_path = entry.relative_path.as_str();
                let basename = entry.basename.as_str();

                // Check exclude pattern
                // If the user searches for the exact file name, we adjust the glob matching
                // to ignore filtering by siblings because the user seems to know what they
                // are searching for and we want to include the result in that case anyway
                let sibling = if !strings_equals(file_pattern, Some(basename), ignore_glob_case) { has_sibling.as_deref() } else { None };
                if query_tester.matches_excludes_sync(relative_path, Some(basename), sibling) {
                    continue;
                }

                if let Some(sub) = path_to_entries.get(relative_path) {
                    match_directory(engine, path_to_entries, sub, query_tester, file_pattern, ignore_glob_case, on_result);
                } else {
                    if strings_equals(Some(relative_path), file_pattern, ignore_glob_case) {
                        continue; // ignore file if its path matches with the file pattern because that is already matched above
                    }

                    engine.match_file(on_result, entry.as_match());
                }

                if engine.is_limit_hit.get() {
                    break;
                }
            }
        }

        if let Some(root_entries) = tree.path_to_entries.get(".") {
            match_directory(self, &tree.path_to_entries, root_entries, query_tester, file_pattern, ignore_glob_case, on_result);
        }
    }

    fn match_file(&self, on_result: &ResultCallback, candidate: IInternalFileMatch) {
        let matches_include = match &self.include_pattern {
            None => true,
            Some(include) => candidate
                .relative_path
                .as_deref()
                .is_some_and(|relative_path| !relative_path.is_empty() && expr_match_is_truthy(&include.matches(relative_path, Some(&candidate.basename), None))),
        };
        if matches_include {
            if self.exists.unwrap_or(false) || self.max_results.is_some_and(|max_results| self.result_count.get() >= max_results) {
                self.is_limit_hit.set(true);
                self.cancel();
            }

            if !self.is_limit_hit.get() {
                on_result(candidate);
            }
        }
    }
}

/// For backwards compatibility, store both a cancellation token and a session object. The session object is the new implementation, where
struct SessionLifecycle {
    obj: RefCell<Option<Rc<dyn Any>>>,
    token_source: CancellationTokenSource,
}

impl SessionLifecycle {
    fn new() -> Self {
        Self { obj: RefCell::new(Some(Rc::new(()))), token_source: CancellationTokenSource::new(None) }
    }

    fn obj(&self) -> Result<Rc<dyn Any>, Error> {
        match self.obj.borrow().as_ref() {
            Some(obj) => Ok(obj.clone()),
            None => Err(Error::new("Session object has been dereferenced.")),
        }
    }

    /// O `session` das opções do provedor: o token da sessão para o provedor antigo, o objeto para o novo.
    fn session(&self, old_provider: bool) -> Result<Rc<dyn Any>, Error> {
        if old_provider { Ok(Rc::new(self.token_source.token())) } else { self.obj() }
    }

    fn cancel(&self) {
        self.token_source.cancel();
        *self.obj.borrow_mut() = None; // dereference
    }
}

/// `FileSearchManager`.
#[derive(Default)]
pub struct FileSearchManager {
    sessions: RefCell<HashMap<String, Rc<SessionLifecycle>>>,
}

impl FileSearchManager {
    const BATCH_SIZE: usize = 512;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn file_search(&self, config: &IFileQuery, provider: Rc<dyn FileSearchProvider2>, on_batch: Rc<dyn Fn(Vec<IFileMatch>)>, token: &CancellationToken) -> Promise<ISearchCompleteStats> {
        let session_token_source = self.get_session_token_source(config.cache_key.as_deref());
        let engine = FileSearchEngine::new(config.clone(), provider, session_token_source);

        let result_count = Rc::new(Cell::new(0usize));
        let on_internal_result: Rc<dyn Fn(Vec<IInternalFileMatch>)> = {
            let result_count = result_count.clone();
            Rc::new(move |batch: Vec<IInternalFileMatch>| {
                result_count.set(result_count.get() + batch.len());
                on_batch(batch.iter().map(Self::raw_match_to_search_item).collect());
            })
        };

        Self::do_search(&engine, Self::BATCH_SIZE, on_internal_result, token).then(move |result| ISearchCompleteStats {
            limit_hit: Some(result.limit_hit),
            stats: result.stats.map(|detail_stats| {
                ISearchStats::File(IFileSearchStats {
                    from_cache: false,
                    stats_type: FileSearchStatsType::FileSearchProvider,
                    result_count: result_count.get() as f64,
                    detail_stats: FileSearchDetailStats::Provider(detail_stats),
                    sorting_time: None,
                })
            }),
            messages: Vec::new(),
        })
    }

    pub fn clear_cache(&self, cache_key: &str) {
        // cancel the token
        if let Some(session) = self.sessions.borrow().get(cache_key) {
            session.cancel();
        }
        // with no reference to this, it will be removed from WeakMaps
        self.sessions.borrow_mut().remove(cache_key);
    }

    fn get_session_token_source(&self, cache_key: Option<&str>) -> Option<Rc<SessionLifecycle>> {
        let cache_key = cache_key.filter(|cache_key| !cache_key.is_empty())?;

        Some(self.sessions.borrow_mut().entry(cache_key.to_owned()).or_insert_with(|| Rc::new(SessionLifecycle::new())).clone())
    }

    fn raw_match_to_search_item(match_: &IInternalFileMatch) -> IFileMatch {
        match match_.relative_path.as_deref().filter(|relative_path| !relative_path.is_empty()) {
            Some(relative_path) => IFileMatch::new(join_path(&match_.base, &[relative_path]), None),
            // extraFileResources
            None => IFileMatch::new(match_.base.clone(), None),
        }
    }

    fn do_search(engine: &Rc<FileSearchEngine>, batch_size: usize, on_result_batch: Rc<dyn Fn(Vec<IInternalFileMatch>)>, token: &CancellationToken) -> Promise<IInternalSearchComplete> {
        let cancel_engine = engine.clone();
        let listener = token.on_cancellation_requested().listen(move |_| cancel_engine.cancel());

        let batch: Rc<RefCell<Vec<IInternalFileMatch>>> = Rc::new(RefCell::new(Vec::new()));
        let on_result: ResultCallback = {
            let batch = batch.clone();
            let on_result_batch = on_result_batch.clone();
            Rc::new(move |match_: IInternalFileMatch| {
                batch.borrow_mut().push(match_);
                if batch_size > 0 && batch.borrow().len() >= batch_size {
                    let full = batch.take();
                    on_result_batch(full);
                }
            })
        };

        engine.search(on_result).then_result(move |result| {
            if !batch.borrow().is_empty() {
                let rest = batch.take();
                on_result_batch(rest);
            }

            listener.dispose();
            result
        })
    }
}
