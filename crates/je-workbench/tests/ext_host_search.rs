//! Porte de `src/vs/workbench/api/test/node/extHostSearch.test.ts`.
//!
//! Suíte `ExtHostSearch`, com as subsuítes `File:` e `Text:`, caso a caso e na mesma ordem. Os 32 casos do original: 17 de busca
//! de arquivo (prefixo `file_`) e 15 de busca de texto (prefixo `text_`).
//!
//! Notas de tradução:
//! - `new TestRPCProtocol()` é o [`TestRpcProtocol`] (`support/test_rpc_protocol_deferred.rs`): o [`MainThreadSearchProxy`]
//!   encaminha cada método de `MainThreadSearchShape` por `remote_call` ao [`MockMainThreadSearch`] que `set` registrou (o `Proxy`
//!   do original, que faz isso para qualquer `$método`), e `rpcProtocol.sync()` é o `sync()` do protocolo de teste.
//! - `MockMainThreadSearch.results: Array<UriComponents | IRawFileMatch2>` guarda dois tipos de resultado em uma lista; aqui são
//!   duas listas (`file_results` para `$handleFileMatch`, `text_results` para `$handleTextMatch`), porque cada caso usa uma só.
//!   `revive(results)` é a conversão do `IRawFileMatch2` em `IFileMatch`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no `setup` de cada caso (o `Fixture`); o `teardown`
//!   (`rpcProtocol.sync()`) é [`Fixture::teardown`], chamado no fim do corpo. O corpo roda em [`run_with_faked_timers`].
//! - `new class extends NativeExtHostSearch { ... }` com `mock<IExtHostInitDataService>`, `new URITransformerService(null)` e o
//!   `IExtHostConfiguration` de provedor vazio são o [`NativeExtHostSearch::new`] com `ExtHostSearchInitData` sem remoto, o
//!   [`TestUriTransformer`] (identidade) e o [`TestSearchConfiguration`]. O `RipgrepSearchProvider` e o `SearchService`, que o
//!   original constrói de verdade e nenhum caso chama, são os dublês de [`test_backends`] (que falham se usados). `this._pfs =
//!   mockPFS` e `createTextSearchManager` são o [`NativeExtHostSearch::set_readdir`]: o `(mockPFS as any).Promises = { readdir }` dos
//!   casos de irmãos é o `set_readdir` do caso.
//! - Os provedores literais (`{ provideFileSearchResults(...) {...} }`) são [`file_provider`] e [`text_provider`], que recebem o
//!   fecho do corpo. `Promise.resolve(null!)` é `Promise::resolved(None)`; `process.nextTick(r)` é [`next_tick`] (uma microtarefa).
//!   `disposables.add(token.onCancellationRequested(...))` é `store.add(token.on_cancellation_requested().listen(...))`.
//! - `new Range(...)` é o `Range` de `searchExtTypes`; `makePreview`, `makeTextResult` e `assertResults` seguem o original. O
//!   `localeCompare` que ordena os resultados antes da comparação é a comparação de texto: os dois lados são ordenados com a mesma
//!   função, então qualquer ordem total serve.
//! - `compareURIs` ordena o `toString()` de cada URI, que é o que `arr.sort()` sobre `URI` faz (o `String(uri)`).
//! - `provider returns null` (arquivo) e `provider fail` (texto): o `assert(false, 'Expected to fail')` do original fica dentro do
//!   `try` e é engolido pelo próprio `catch {}`, então o caso passa com ou sem falha da busca; o porte executa a busca e ignora o
//!   resultado, como o original. O provedor que devolve `null` sem promessa dá, no conversor, a lista vazia (a mesma de
//!   `Promise::resolved(None)`), e o `throw` do provedor de texto é `errors::throw`.
//! - `new Promise(r => process.nextTick(r))` dos provedores assíncronos de vários roots dá ao motor a chance de invocar todos os
//!   provedores antes do primeiro resultado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

#[path = "support/test_rpc_protocol_deferred.rs"]
mod test_rpc_protocol_deferred;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::glob::{ExprValue, Expression};
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::{URI, UriComponents};
use je_platform::log::NullLogService;
use je_platform::progress::progress::IProgress;
use je_platform::resources::{fs_path, join_path, uri_file};
use je_workbench::api::common::ext_host_protocol::{ExtHostSearchShape, IRawFileMatch2, MainContext, MainThreadSearchShape};
use je_workbench::api::common::ext_host_workspace::IURITransformerService;
use je_workbench::api::node::ext_host_search::{
    ExtHostSearchInitData, IExtHostSearchConfigProvider, IExtHostSearchConfiguration, IInternalFileSearchService, NativeExtHostSearch, NativeSearchBackends, NumThreadsFn,
    SerializedSearchProgressItem, ConfigChangeListener,
};
use je_workbench::services::search::ripgrep_search_utils::OutputChannel;
use je_workbench::services::search::search::{
    ExcludeGlobPattern, ICommonQueryProps, IFileMatch, IFileQuery, IFolderQuery, ISearchCompleteStats, ITextQuery, ITextSearchResult, PatternInfo,
};
use je_workbench::services::search::search_ext_conversion_types::{
    FileSearchOptions, FileSearchProvider, FileSearchQuery, RangeOrRanges, TextSearchComplete, TextSearchContext, TextSearchMatch, TextSearchMatchPreview, TextSearchOptions,
    TextSearchProvider, TextSearchQuery, TextSearchResult,
};
use je_workbench::services::search::search_ext_types::{AISearchKeyword, ProviderResult, Range, TextSearchComplete2, TextSearchProvider2, TextSearchProviderOptions, TextSearchQuery2, TextSearchResult2};
use test_rpc_protocol_deferred::TestRpcProtocol;

// ---------------------------------------------------------------------------------------------------------
// MockMainThreadSearch e o proxy do protocolo de teste
// ---------------------------------------------------------------------------------------------------------

/// `MockMainThreadSearch`.
#[derive(Default)]
struct MockMainThreadSearch {
    last_handle: Cell<u64>,
    file_results: RefCell<Vec<URI>>,
    text_results: RefCell<Vec<IRawFileMatch2>>,
    keywords: RefCell<Vec<AISearchKeyword>>,
}

impl IDisposable for MockMainThreadSearch {
    fn dispose(&self) {}
}

impl MainThreadSearchShape for MockMainThreadSearch {
    fn rpc_register_file_search_provider(&self, handle: u64, _scheme: &str) {
        self.last_handle.set(handle);
    }

    fn rpc_register_ai_text_search_provider(&self, handle: u64, _scheme: &str) {
        self.last_handle.set(handle);
    }

    fn rpc_register_text_search_provider(&self, handle: u64, _scheme: &str) {
        self.last_handle.set(handle);
    }

    fn rpc_unregister_provider(&self, _handle: u64) {}

    fn rpc_handle_file_match(&self, _handle: u64, _session: u64, data: Vec<URI>) {
        self.file_results.borrow_mut().extend(data);
    }

    fn rpc_handle_text_match(&self, _handle: u64, _session: u64, data: Vec<IRawFileMatch2>) {
        self.text_results.borrow_mut().extend(data);
    }

    fn rpc_handle_keyword_result(&self, _handle: u64, _session: u64, data: AISearchKeyword) {
        self.keywords.borrow_mut().push(data);
    }

    fn rpc_handle_telemetry(&self, _event_name: &str, _data: Dynamic) {}
}

/// O proxy de `MainContext.MainThreadSearch`: cada método vira uma chamada remota no objeto que `set` registrou.
struct MainThreadSearchProxy {
    rpc: Rc<TestRpcProtocol>,
}

impl MainThreadSearchProxy {
    fn forward(&self, call: impl FnOnce(Rc<dyn MainThreadSearchShape>) + 'static) {
        let rpc = Rc::clone(&self.rpc);
        drop(self.rpc.remote_call(move || {
            call(rpc.local(&MainContext::MAIN_THREAD_SEARCH));
            Promise::resolved(())
        }));
    }
}

impl IDisposable for MainThreadSearchProxy {
    fn dispose(&self) {}
}

impl MainThreadSearchShape for MainThreadSearchProxy {
    fn rpc_register_file_search_provider(&self, handle: u64, scheme: &str) {
        let scheme = scheme.to_owned();
        self.forward(move |local| local.rpc_register_file_search_provider(handle, &scheme));
    }

    fn rpc_register_ai_text_search_provider(&self, handle: u64, scheme: &str) {
        let scheme = scheme.to_owned();
        self.forward(move |local| local.rpc_register_ai_text_search_provider(handle, &scheme));
    }

    fn rpc_register_text_search_provider(&self, handle: u64, scheme: &str) {
        let scheme = scheme.to_owned();
        self.forward(move |local| local.rpc_register_text_search_provider(handle, &scheme));
    }

    fn rpc_unregister_provider(&self, handle: u64) {
        self.forward(move |local| local.rpc_unregister_provider(handle));
    }

    fn rpc_handle_file_match(&self, handle: u64, session: u64, data: Vec<URI>) {
        self.forward(move |local| local.rpc_handle_file_match(handle, session, data));
    }

    fn rpc_handle_text_match(&self, handle: u64, session: u64, data: Vec<IRawFileMatch2>) {
        self.forward(move |local| local.rpc_handle_text_match(handle, session, data));
    }

    fn rpc_handle_keyword_result(&self, handle: u64, session: u64, data: AISearchKeyword) {
        self.forward(move |local| local.rpc_handle_keyword_result(handle, session, data));
    }

    fn rpc_handle_telemetry(&self, event_name: &str, data: Dynamic) {
        let event_name = event_name.to_owned();
        self.forward(move |local| local.rpc_handle_telemetry(&event_name, data));
    }
}

// ---------------------------------------------------------------------------------------------------------
// Dublês das dependências do NativeExtHostSearch
// ---------------------------------------------------------------------------------------------------------

/// `new URITransformerService(null)`: sem transformador, o esquema sai como entrou.
struct TestUriTransformer;

impl IURITransformerService for TestUriTransformer {
    fn transform_outgoing_scheme(&self, scheme: &str) -> String {
        scheme.to_owned()
    }
}

/// O `getConfiguration()` do original: `get() {}`, `has() { return false; }`, `inspect() { return undefined; }`.
struct TestSearchConfigProvider;

impl IExtHostSearchConfigProvider for TestSearchConfigProvider {
    fn on_did_change_configuration(&self, _listener: ConfigChangeListener) -> DisposableRef {
        to_disposable(|| {})
    }

    fn get_ripgrep_max_threads(&self) -> Option<i64> {
        None
    }
}

/// `new class extends mock<IExtHostConfiguration>() { override async getConfigProvider() { ... } }`.
struct TestSearchConfiguration;

impl IExtHostSearchConfiguration for TestSearchConfiguration {
    fn get_config_provider(&self) -> Promise<Rc<dyn IExtHostSearchConfigProvider>> {
        Promise::resolved(Rc::new(TestSearchConfigProvider) as Rc<dyn IExtHostSearchConfigProvider>)
    }
}

/// O `RipgrepSearchProvider` real, que nenhum caso chama.
struct UnusedRipgrepSearchProvider;

impl TextSearchProvider2 for UnusedRipgrepSearchProvider {
    fn provide_text_search_results(
        &self,
        _query: &TextSearchQuery2,
        _options: &TextSearchProviderOptions,
        _progress: Rc<dyn IProgress<TextSearchResult2>>,
        _token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete2> {
        panic!("o RipgrepSearchProvider não é usado por este teste (nenhum caso busca no esquema vscode-userdata)")
    }
}

/// O `SearchService` real, que nenhum caso chama (o host não é remoto e `$enableExtensionHostSearch` não é chamado).
struct UnusedInternalFileSearchService;

impl IInternalFileSearchService for UnusedInternalFileSearchService {
    fn do_file_search(&self, _query: &IFileQuery, _num_threads: Option<i64>, _on_result: Rc<dyn Fn(SerializedSearchProgressItem)>, _token: &CancellationToken) -> Promise<ISearchCompleteStats> {
        panic!("o SearchService não é usado por este teste")
    }

    fn clear_cache(&self, _cache_key: &str) {
        panic!("o SearchService não é usado por este teste")
    }
}

fn test_backends() -> NativeSearchBackends {
    NativeSearchBackends {
        create_ripgrep_search_provider: Rc::new(|_: OutputChannel, _: NumThreadsFn| Rc::new(UnusedRipgrepSearchProvider) as Rc<dyn TextSearchProvider2>),
        create_internal_file_search_service: Rc::new(|_: &str, _: NumThreadsFn| Rc::new(UnusedInternalFileSearchService) as Rc<dyn IInternalFileSearchService>),
    }
}

// ---------------------------------------------------------------------------------------------------------
// setup, teardown e as funções do corpo da suíte
// ---------------------------------------------------------------------------------------------------------

/// O que o `setup` monta e o `teardown` desfaz.
struct Fixture {
    rpc: Rc<TestRpcProtocol>,
    main_thread_search: Rc<MockMainThreadSearch>,
    ext_host_search: Rc<NativeExtHostSearch>,
    /// O `disposables` da suíte (o store do guarda de vazamentos).
    store: Rc<DisposableStore>,
    /// Por último: o guarda confere os vazamentos depois de tudo o mais ter sido liberado.
    _guard: LeakGuard,
}

fn setup() -> Fixture {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let rpc = TestRpcProtocol::new();
    rpc.register_proxy(&MainContext::MAIN_THREAD_SEARCH, Rc::new(MainThreadSearchProxy { rpc: Rc::clone(&rpc) }) as Rc<dyn MainThreadSearchShape>);

    let main_thread_search = Rc::new(MockMainThreadSearch::default());
    rpc.set(&MainContext::MAIN_THREAD_SEARCH, main_thread_search.clone() as Rc<dyn MainThreadSearchShape>);

    let ext_host_search = guard.add_rc(NativeExtHostSearch::new(
        rpc.as_ref(),
        &ExtHostSearchInitData::default(),
        Rc::new(TestUriTransformer),
        Rc::new(TestSearchConfiguration),
        NullLogService::shared(),
        test_backends(),
    ));
    let store = guard.store();
    Fixture { rpc, main_thread_search, ext_host_search, store, _guard: guard }
}

/// O resultado de `runFileSearch` e `runTextSearch`: o `stats` é `undefined` quando a busca foi cancelada.
struct SearchRun<R> {
    results: Vec<R>,
    stats: Option<ISearchCompleteStats>,
}

impl<R> SearchRun<R> {
    /// `stats.limitHit`.
    fn limit_hit(&self) -> bool {
        self.stats.as_ref().expect("stats").limit_hit.unwrap_or(false)
    }
}

impl Fixture {
    /// `teardown(() => rpcProtocol.sync())`.
    async fn teardown(&self) {
        self.rpc.sync().await.expect("sync");
    }

    /// `registerTestTextSearchProvider`.
    async fn register_test_text_search_provider(&self, provider: Rc<dyn TextSearchProvider>, scheme: &str) {
        let registration = self.ext_host_search.register_text_search_provider_old(scheme, provider).expect("registerTextSearchProviderOld");
        self.store.add(registration);
        self.rpc.sync().await.expect("sync");
    }

    /// `registerTestFileSearchProvider`.
    async fn register_test_file_search_provider(&self, provider: Rc<dyn FileSearchProvider>, scheme: &str) {
        let registration = self.ext_host_search.register_file_search_provider_old(scheme, provider).expect("registerFileSearchProviderOld");
        self.store.add(registration);
        self.rpc.sync().await.expect("sync");
    }

    /// `runFileSearch(query, cancel)`. Um erro de cancelamento deixa o `stats` sem valor; qualquer outro erro é devolvido depois do
    /// `sync`.
    async fn run_file_search(&self, query: IFileQuery, cancel: bool) -> Result<SearchRun<URI>, Error> {
        let cancellation = CancellationTokenSource::new(None);
        let p = self.ext_host_search.rpc_provide_file_search_results(self.main_thread_search.last_handle.get(), 0, query, cancellation.token());
        if cancel {
            timeout(0).await?;
            cancellation.cancel();
        }

        let stats = match p.await {
            Ok(stats) => Some(stats),
            Err(err) if is_cancellation_error(&err) => None,
            Err(err) => {
                self.rpc.sync().await?;
                return Err(err);
            }
        };

        self.rpc.sync().await?;
        Ok(SearchRun { results: self.main_thread_search.file_results.borrow().clone(), stats })
    }

    /// `runTextSearch(query)`.
    async fn run_text_search(&self, query: ITextQuery) -> Result<SearchRun<IFileMatch>, Error> {
        let cancellation = CancellationTokenSource::new(None);
        let p = self.ext_host_search.rpc_provide_text_search_results(self.main_thread_search.last_handle.get(), 0, query, cancellation.token());

        let stats = match p.await {
            Ok(stats) => Some(stats),
            Err(err) if is_cancellation_error(&err) => None,
            Err(err) => {
                self.rpc.sync().await?;
                return Err(err);
            }
        };

        self.rpc.sync().await?;
        let results = self.main_thread_search.text_results.borrow().iter().map(|raw| IFileMatch::new(raw.resource.clone(), raw.results.clone())).collect();
        Ok(SearchRun { results, stats })
    }
}

fn root_folder_a() -> URI {
    uri_file("/foo/bar1")
}

fn root_folder_b() -> URI {
    uri_file("/foo/bar2")
}

const FANCY_SCHEME: &str = "fancy";

fn fancy_scheme_folder_a() -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(FANCY_SCHEME)), path: Some(JsString::from("/project/folder1")), ..UriComponents::default() }, false)
}

/// `process.nextTick(r)` dentro de `new Promise(...)`.
fn next_tick() -> Promise<()> {
    Promise::with_executor(|resolver| runtime::queue_microtask(move || resolver.resolve(())))
}

const ON: ExprValue = ExprValue::Bool(true);
const OFF: ExprValue = ExprValue::Bool(false);

/// `{ when: '$(basename).ts' }`.
fn when(text: &str) -> ExprValue {
    ExprValue::When(text.to_owned())
}

/// O objeto `{ pattern: value, ... }` de um `IExpression`.
fn expr(entries: Vec<(&str, ExprValue)>) -> Expression {
    Expression::from_entries(entries)
}

/// `[{ pattern }]` do `excludePattern` de uma pasta.
fn folder_excludes(pattern: Expression) -> Vec<ExcludeGlobPattern> {
    vec![ExcludeGlobPattern { folder: None, pattern }]
}

fn folder_query(folder: URI) -> IFolderQuery {
    IFolderQuery::new(folder)
}

/// `options.includes.sort()`.
fn sorted(mut patterns: Vec<String>) -> Vec<String> {
    patterns.sort();
    patterns
}

fn is_folder(options_folder: &URI, folder: &URI) -> bool {
    options_folder.to_string(false).to_string_lossy() == folder.to_string(false).to_string_lossy()
}

// Os provedores literais do original.

struct FileProviderFn<F>(F);

impl<F> FileSearchProvider for FileProviderFn<F>
where
    F: Fn(&FileSearchQuery, &FileSearchOptions, &CancellationToken) -> ProviderResult<Vec<URI>>,
{
    fn provide_file_search_results(&self, query: &FileSearchQuery, options: &FileSearchOptions, token: &CancellationToken) -> ProviderResult<Vec<URI>> {
        (self.0)(query, options, token)
    }
}

/// `{ provideFileSearchResults(query, options, token) { ... } }`.
fn file_provider<F>(provide: F) -> Rc<dyn FileSearchProvider>
where
    F: Fn(&FileSearchQuery, &FileSearchOptions, &CancellationToken) -> ProviderResult<Vec<URI>> + 'static,
{
    Rc::new(FileProviderFn(provide))
}

struct TextProviderFn<F>(F);

impl<F> TextSearchProvider for TextProviderFn<F>
where
    F: Fn(&TextSearchQuery, &TextSearchOptions, Rc<dyn IProgress<TextSearchResult>>, &CancellationToken) -> ProviderResult<TextSearchComplete>,
{
    fn provide_text_search_results(
        &self,
        query: &TextSearchQuery,
        options: &TextSearchOptions,
        progress: Rc<dyn IProgress<TextSearchResult>>,
        token: &CancellationToken,
    ) -> ProviderResult<TextSearchComplete> {
        (self.0)(query, options, progress, token)
    }
}

/// `{ provideTextSearchResults(query, options, progress, token) { ... } }`.
fn text_provider<F>(provide: F) -> Rc<dyn TextSearchProvider>
where
    F: Fn(&TextSearchQuery, &TextSearchOptions, Rc<dyn IProgress<TextSearchResult>>, &CancellationToken) -> ProviderResult<TextSearchComplete> + 'static,
{
    Rc::new(TextProviderFn(provide))
}

// ---------------------------------------------------------------------------------------------------------
// File:
// ---------------------------------------------------------------------------------------------------------

/// `getSimpleQuery(filePattern)` da subsuíte `File:`.
fn simple_file_query(file_pattern: &str) -> IFileQuery {
    file_query(file_pattern, ICommonQueryProps::new(vec![folder_query(root_folder_a())]))
}

/// Uma `IFileQuery` com o padrão de arquivo e as partes comuns dadas.
fn file_query(file_pattern: &str, common: ICommonQueryProps) -> IFileQuery {
    IFileQuery { common, file_pattern: Some(file_pattern.to_owned()), ..IFileQuery::new(Vec::new()) }
}

/// `compareURIs(actual, expected)`.
fn compare_uris(actual: &[URI], expected: &[URI]) {
    let sort_and_stringify = |uris: &[URI]| {
        let mut texts: Vec<String> = uris.iter().map(|uri| uri.to_string(false).to_string_lossy()).collect();
        texts.sort();
        texts
    };

    assert_eq!(sort_and_stringify(actual), sort_and_stringify(expected));
}

#[test]
fn file_no_results() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_file_search_provider(file_provider(|_, _, _| Promise::resolved(None)), "file").await;

        let run = fx.run_file_search(simple_file_query(""), false).await.expect("runFileSearch");
        assert!(!run.limit_hit());
        assert!(run.results.is_empty());
        fx.teardown().await;
    });
}

#[test]
fn file_simple_results() {
    run_with_faked_timers(async {
        let fx = setup();
        let reported_results = vec![join_path(&root_folder_a(), &["file1.ts"]), join_path(&root_folder_a(), &["file2.ts"]), join_path(&root_folder_a(), &["subfolder/file3.ts"])];

        let reported = reported_results.clone();
        fx.register_test_file_search_provider(file_provider(move |_, _, _| Promise::resolved(Some(reported.clone()))), "file").await;

        let run = fx.run_file_search(simple_file_query(""), false).await.expect("runFileSearch");
        assert!(!run.limit_hit());
        assert_eq!(run.results.len(), 3);
        compare_uris(&run.results, &reported_results);
        fx.teardown().await;
    });
}

#[test]
fn file_search_canceled() {
    run_with_faked_timers(async {
        let fx = setup();
        let cancel_requested = Rc::new(Cell::new(false));
        let (flag, store) = (cancel_requested.clone(), fx.store.clone());
        fx.register_test_file_search_provider(
            file_provider(move |_, options, token| {
                let (cancel_requested, store, token, folder) = (flag.clone(), store.clone(), token.clone(), options.common.folder.clone());
                Promise::with_executor(move |resolver| {
                    let on_cancel = move || {
                        cancel_requested.set(true);

                        resolver.resolve(Some(vec![join_path(&folder, &["file1.ts"])])); // or reject or nothing?
                    };

                    if token.is_cancellation_requested() {
                        on_cancel();
                    } else {
                        store.add(token.on_cancellation_requested().listen(move |_| on_cancel()));
                    }
                })
            }),
            "file",
        )
        .await;

        let run = fx.run_file_search(simple_file_query(""), true).await.expect("runFileSearch");
        assert!(cancel_requested.get());
        assert!(run.results.is_empty());
        fx.teardown().await;
    });
}

#[test]
fn file_session_cancellation_should_work() {
    run_with_faked_timers(async {
        let fx = setup();
        let num_session_cancelled = Rc::new(Cell::new(0));
        let disposables: Rc<RefCell<Vec<Option<DisposableRef>>>> = Rc::default();
        let (cancelled, listeners) = (num_session_cancelled.clone(), disposables.clone());
        fx.register_test_file_search_provider(
            file_provider(move |_, options, _| {
                let cancelled = cancelled.clone();
                listeners.borrow_mut().push(options.session.as_ref().map(|session| session.on_cancellation_requested().listen(move |_| cancelled.set(cancelled.get() + 1))));

                Promise::resolved(Some(Vec::new()))
            }),
            "file",
        )
        .await;

        fx.run_file_search(IFileQuery { cache_key: Some("1".to_owned()), ..simple_file_query("") }, true).await.expect("runFileSearch");
        fx.run_file_search(IFileQuery { cache_key: Some("2".to_owned()), ..simple_file_query("") }, true).await.expect("runFileSearch");
        drop(fx.ext_host_search.rpc_clear_cache("1"));
        assert_eq!(num_session_cancelled.get(), 1);
        for disposable in disposables.borrow().iter().flatten() {
            disposable.dispose();
        }
        fx.teardown().await;
    });
}

#[test]
fn file_provider_returns_null() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_file_search_provider(file_provider(|_, _, _| Promise::resolved(None)), "file").await;

        // O `assert(false, 'Expected to fail')` do original é engolido pelo `catch {}` do mesmo `try`: o caso passa com ou sem erro.
        let _ = fx.run_file_search(simple_file_query(""), false).await;
        fx.teardown().await;
    });
}

#[test]
fn file_all_provider_calls_get_global_include_excludes() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_file_search_provider(
            file_provider(|_, options, _| {
                assert!(options.common.excludes.len() == 2 && options.common.includes.len() == 2, "Missing global include/excludes");
                Promise::resolved(None)
            }),
            "file",
        )
        .await;

        let query = file_query(
            "",
            ICommonQueryProps {
                include_pattern: Some(expr(vec![("foo", ON), ("bar", ON)])),
                exclude_pattern: Some(expr(vec![("something", ON), ("else", ON)])),
                ..ICommonQueryProps::new(vec![folder_query(root_folder_a()), folder_query(root_folder_b())])
            },
        );

        fx.run_file_search(query, false).await.expect("runFileSearch");
        fx.teardown().await;
    });
}

#[test]
fn file_global_local_include_excludes_combined() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_file_search_provider(
            file_provider(|_, options, _| {
                if is_folder(&options.common.folder, &root_folder_a()) {
                    assert_eq!(sorted(options.common.includes.clone()), ["*.ts", "foo"]);
                    assert_eq!(sorted(options.common.excludes.clone()), ["*.js", "bar"]);
                } else {
                    assert_eq!(sorted(options.common.includes.clone()), ["*.ts"]);
                    assert_eq!(sorted(options.common.excludes.clone()), ["*.js"]);
                }

                Promise::resolved(None)
            }),
            "file",
        )
        .await;

        let query = file_query(
            "",
            ICommonQueryProps {
                include_pattern: Some(expr(vec![("*.ts", ON)])),
                exclude_pattern: Some(expr(vec![("*.js", ON)])),
                ..ICommonQueryProps::new(vec![
                    IFolderQuery {
                        include_pattern: Some(expr(vec![("foo", ON)])),
                        exclude_pattern: Some(folder_excludes(expr(vec![("bar", ON)]))),
                        ..folder_query(root_folder_a())
                    },
                    folder_query(root_folder_b()),
                ])
            },
        );

        fx.run_file_search(query, false).await.expect("runFileSearch");
        fx.teardown().await;
    });
}

#[test]
fn file_include_excludes_resolved_correctly() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_file_search_provider(
            file_provider(|_, options, _| {
                assert_eq!(sorted(options.common.includes.clone()), ["*.jsx", "*.ts"]);
                assert_eq!(sorted(options.common.excludes.clone()), Vec::<String>::new());

                Promise::resolved(None)
            }),
            "file",
        )
        .await;

        let query = file_query(
            "",
            ICommonQueryProps {
                include_pattern: Some(expr(vec![("*.ts", ON), ("*.jsx", OFF)])),
                exclude_pattern: Some(expr(vec![("*.js", ON), ("*.tsx", OFF)])),
                ..ICommonQueryProps::new(vec![IFolderQuery {
                    include_pattern: Some(expr(vec![("*.jsx", ON)])),
                    exclude_pattern: Some(folder_excludes(expr(vec![("*.js", OFF)]))),
                    ..folder_query(root_folder_a())
                }])
            },
        );

        fx.run_file_search(query, false).await.expect("runFileSearch");
        fx.teardown().await;
    });
}

#[test]
fn file_basic_sibling_exclude_clause() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_file_search_provider(
            file_provider(|_, options, _| Promise::resolved(Some(["file1.ts", "file1.js"].into_iter().map(|relative_path| join_path(&options.common.folder, &[relative_path])).collect()))),
            "file",
        )
        .await;

        let query = file_query(
            "",
            ICommonQueryProps { exclude_pattern: Some(expr(vec![("*.js", when("$(basename).ts"))])), ..ICommonQueryProps::new(vec![folder_query(root_folder_a())]) },
        );

        let run = fx.run_file_search(query, false).await.expect("runFileSearch");
        compare_uris(&run.results, &[join_path(&root_folder_a(), &["file1.ts"])]);
        fx.teardown().await;
    });
}

// https://github.com/microsoft/vscode-remotehub/issues/255
#[test]
fn file_include_sibling_exclude_and_subfolder() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_file_search_provider(
            file_provider(|_, options, _| Promise::resolved(Some(["foo/file1.ts", "foo/file1.js"].into_iter().map(|relative_path| join_path(&options.common.folder, &[relative_path])).collect()))),
            "file",
        )
        .await;

        let query = file_query(
            "",
            ICommonQueryProps {
                include_pattern: Some(expr(vec![("**/*.ts", ON)])),
                exclude_pattern: Some(expr(vec![("*.js", when("$(basename).ts"))])),
                ..ICommonQueryProps::new(vec![folder_query(root_folder_a())])
            },
        );

        let run = fx.run_file_search(query, false).await.expect("runFileSearch");
        compare_uris(&run.results, &[join_path(&root_folder_a(), &["foo/file1.ts"])]);
        fx.teardown().await;
    });
}

#[test]
fn file_multiroot_sibling_exclude_clause() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_file_search_provider(
            file_provider(|_, options, _| {
                let (root, relative_paths) = if fs_path(&options.common.folder) == fs_path(&root_folder_a()) {
                    (root_folder_a(), ["folder/fileA.scss", "folder/fileA.css", "folder/file2.css"])
                } else {
                    (root_folder_b(), ["fileB.ts", "fileB.js", "file3.js"])
                };

                Promise::resolved(Some(relative_paths.into_iter().map(|relative_path| join_path(&root, &[relative_path])).collect()))
            }),
            "file",
        )
        .await;

        let query = file_query(
            "",
            ICommonQueryProps {
                exclude_pattern: Some(expr(vec![("*.js", when("$(basename).ts")), ("*.css", ON)])),
                ..ICommonQueryProps::new(vec![
                    IFolderQuery { exclude_pattern: Some(folder_excludes(expr(vec![("folder/*.css", when("$(basename).scss"))]))), ..folder_query(root_folder_a()) },
                    IFolderQuery { exclude_pattern: Some(folder_excludes(expr(vec![("*.js", OFF)]))), ..folder_query(root_folder_b()) },
                ])
            },
        );

        let run = fx.run_file_search(query, false).await.expect("runFileSearch");
        compare_uris(
            &run.results,
            &[
                join_path(&root_folder_a(), &["folder/fileA.scss"]),
                join_path(&root_folder_a(), &["folder/file2.css"]),
                join_path(&root_folder_b(), &["fileB.ts"]),
                join_path(&root_folder_b(), &["fileB.js"]),
                join_path(&root_folder_b(), &["file3.js"]),
            ],
        );
        fx.teardown().await;
    });
}

/// O corpo de `max results = N` e de `provider returns maxResults exactly` da subsuíte `File:`: o provedor devolve `reported`, a
/// consulta pede `max_results`, e o caso confere o `limitHit`, os resultados e se o provedor foi cancelado.
async fn file_max_results_case(reported: Vec<URI>, max_results: i64, expect_limit_hit: bool, expected_count: usize, expect_canceled: bool) {
    let fx = setup();
    let was_canceled = Rc::new(Cell::new(false));
    let (canceled, store, provided) = (was_canceled.clone(), fx.store.clone(), reported.clone());
    fx.register_test_file_search_provider(
        file_provider(move |_, _, token| {
            let canceled = canceled.clone();
            store.add(token.on_cancellation_requested().listen(move |_| canceled.set(true)));

            Promise::resolved(Some(provided.clone()))
        }),
        "file",
    )
    .await;

    let query = file_query("", ICommonQueryProps { max_results: Some(max_results), ..ICommonQueryProps::new(vec![folder_query(root_folder_a())]) });

    let run = fx.run_file_search(query, false).await.expect("runFileSearch");
    assert_eq!(run.limit_hit(), expect_limit_hit, "limitHit");
    assert_eq!(run.results.len(), expected_count);
    compare_uris(&run.results, &reported[..expected_count]);
    assert_eq!(was_canceled.get(), expect_canceled, "canceled");
    fx.teardown().await;
}

fn three_files() -> Vec<URI> {
    vec![join_path(&root_folder_a(), &["file1.ts"]), join_path(&root_folder_a(), &["file2.ts"]), join_path(&root_folder_a(), &["file3.ts"])]
}

#[test]
fn file_max_results_1() {
    // Expected to return limitHit; Expected to be canceled when hitting limit
    run_with_faked_timers(file_max_results_case(three_files(), 1, true, 1, true));
}

#[test]
fn file_max_results_2() {
    // Expected to return limitHit; Expected to be canceled when hitting limit
    run_with_faked_timers(file_max_results_case(three_files(), 2, true, 2, true));
}

#[test]
fn file_provider_returns_max_results_exactly() {
    // Expected not to return limitHit; Expected not to be canceled when just reaching limit
    run_with_faked_timers(file_max_results_case(three_files()[..2].to_vec(), 2, false, 2, false));
}

#[test]
fn file_multiroot_max_results() {
    run_with_faked_timers(async {
        let fx = setup();
        let cancels = Rc::new(Cell::new(0));
        let (counter, store) = (cancels.clone(), fx.store.clone());
        fx.register_test_file_search_provider(
            file_provider(move |_, options, token| {
                let counter = counter.clone();
                store.add(token.on_cancellation_requested().listen(move |_| counter.set(counter.get() + 1)));

                // Provice results async so it has a chance to invoke every provider
                let folder = options.common.folder.clone();
                async_fn(async move {
                    next_tick().await?;
                    Ok(Some(["file1.ts", "file2.ts", "file3.ts"].into_iter().map(|relative_path| join_path(&folder, &[relative_path])).collect()))
                })
            }),
            "file",
        )
        .await;

        let query = file_query("", ICommonQueryProps { max_results: Some(2), ..ICommonQueryProps::new(vec![folder_query(root_folder_a()), folder_query(root_folder_b())]) });

        let run = fx.run_file_search(query, false).await.expect("runFileSearch");
        assert_eq!(run.results.len(), 2); // Don't care which 2 we got
        assert_eq!(cancels.get(), 2, "Expected all invocations to be canceled when hitting limit");
        fx.teardown().await;
    });
}

#[test]
fn file_works_with_non_file_schemes() {
    run_with_faked_timers(async {
        let fx = setup();
        let reported_results = vec![
            join_path(&fancy_scheme_folder_a(), &["file1.ts"]),
            join_path(&fancy_scheme_folder_a(), &["file2.ts"]),
            join_path(&fancy_scheme_folder_a(), &["subfolder/file3.ts"]),
        ];

        let reported = reported_results.clone();
        fx.register_test_file_search_provider(file_provider(move |_, _, _| Promise::resolved(Some(reported.clone()))), FANCY_SCHEME).await;

        let query = file_query("", ICommonQueryProps::new(vec![folder_query(fancy_scheme_folder_a())]));

        let run = fx.run_file_search(query, false).await.expect("runFileSearch");
        compare_uris(&run.results, &reported_results);
        fx.teardown().await;
    });
}

#[test]
fn file_if_only_file_scheme_is_set_do_not_call_custom_schemes() {
    run_with_faked_timers(async {
        let fx = setup();
        let fancy_scheme_called = Rc::new(Cell::new(false));
        let called = fancy_scheme_called.clone();
        fx.register_test_file_search_provider(
            file_provider(move |_, _, _| {
                called.set(true);
                Promise::resolved(Some(Vec::new()))
            }),
            FANCY_SCHEME,
        )
        .await;

        let query = file_query("", ICommonQueryProps::new(Vec::new()));

        fx.run_file_search(query, false).await.expect("runFileSearch");
        assert!(!fancy_scheme_called.get());
        fx.teardown().await;
    });
}

// ---------------------------------------------------------------------------------------------------------
// Text:
// ---------------------------------------------------------------------------------------------------------

/// O comprimento em unidades UTF-16 (`text.length`).
fn utf16_len(text: &str) -> i64 {
    text.encode_utf16().count() as i64
}

/// `makePreview(text)`.
fn make_preview(text: &str) -> TextSearchMatchPreview {
    TextSearchMatchPreview { matches: RangeOrRanges::Many(vec![Range::new(0, 0, 0, utf16_len(text))]), text: JsString::from(text) }
}

/// `makeTextResult(baseFolder, relativePath)`.
fn make_text_result(base_folder: &URI, relative_path: &str) -> TextSearchResult {
    TextSearchResult::Match(TextSearchMatch { preview: make_preview("foo"), ranges: RangeOrRanges::Many(vec![Range::new(0, 0, 0, 3)]), uri: join_path(base_folder, &[relative_path]) })
}

/// `getSimpleQuery(queryText)` da subsuíte `Text:`.
fn simple_text_query(query_text: &str) -> ITextQuery {
    text_query(query_text, ICommonQueryProps::new(vec![folder_query(root_folder_a())]))
}

/// Uma `ITextQuery` com o padrão `getPattern(queryText)` e as partes comuns dadas.
fn text_query(query_text: &str, common: ICommonQueryProps) -> ITextQuery {
    ITextQuery { common, ..ITextQuery::new(Vec::new(), PatternInfo::new(query_text)) }
}

/// O que `assertResults` compara depois de `makeComparable`.
#[derive(Debug, PartialEq, Eq)]
enum Comparable {
    Match { uri: String, range: Vec<String>, preview_text: String },
    Context { uri: String, text: String, line_number: i64 },
}

/// `rangeToString`.
fn range_to_string(range: &Range) -> String {
    format!("({}, {}), ({}, {})", range.start.line, range.start.character, range.end.line, range.end.character)
}

/// A chave de ordenação de `makeComparable` (`uri.toString() + ': ' + text`).
fn compare_key(comparable: &Comparable) -> String {
    match comparable {
        Comparable::Match { uri, preview_text, .. } => format!("{uri}: {preview_text}"),
        Comparable::Context { uri, text, .. } => format!("{uri}: {text}"),
    }
}

/// `makeComparable(results)`, depois de ordenar do maior para o menor pela chave.
fn make_comparable(mut results: Vec<Comparable>) -> Vec<Comparable> {
    results.sort_by(|a, b| compare_key(b).cmp(&compare_key(a)));
    results
}

/// `assertResults(actual, expected)`.
fn assert_results(actual: &[IFileMatch], expected: &[TextSearchResult]) {
    let mut actual_text_search_results = Vec::new();
    for file_match in actual {
        let uri = file_match.resource.to_string(false).to_string_lossy();
        for line_result in file_match.results.as_ref().expect("fileMatch.results") {
            actual_text_search_results.push(match line_result {
                ITextSearchResult::Match(line_match) => Comparable::Match {
                    uri: uri.clone(),
                    range: line_match
                        .range_locations
                        .iter()
                        .map(|r| range_to_string(&Range::new(r.source.start_line_number, r.source.start_column, r.source.end_line_number, r.source.end_column)))
                        .collect(),
                    preview_text: line_match.preview_text.to_string_lossy(),
                },
                ITextSearchResult::Context(context) => Comparable::Context { uri: uri.clone(), text: context.text.to_string_lossy(), line_number: context.line_number },
            });
        }
    }

    let expected_results = expected
        .iter()
        .map(|result| match result {
            TextSearchResult::Match(text_match) => Comparable::Match {
                uri: text_match.uri.to_string(false).to_string_lossy(),
                range: text_match.ranges.as_array().iter().map(range_to_string).collect(),
                preview_text: text_match.preview.text.to_string_lossy(),
            },
            TextSearchResult::Context(TextSearchContext { uri, text, line_number }) => {
                Comparable::Context { uri: uri.to_string(false).to_string_lossy(), text: text.to_string_lossy(), line_number: *line_number }
            }
        })
        .collect();

    assert_eq!(make_comparable(actual_text_search_results), make_comparable(expected_results));
}

/// O provedor de texto que reporta `provided` e devolve `null`.
fn reporting_text_provider(provided: Vec<TextSearchResult>) -> Rc<dyn TextSearchProvider> {
    text_provider(move |_, _, progress, _| {
        provided.iter().for_each(|result| progress.report(result.clone()));
        Promise::resolved(None)
    })
}

#[test]
fn text_no_results() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_text_search_provider(text_provider(|_, _, _, _| Promise::resolved(None)), "file").await;

        let run = fx.run_text_search(simple_text_query("foo")).await.expect("runTextSearch");
        assert!(!run.limit_hit());
        assert!(run.results.is_empty());
        fx.teardown().await;
    });
}

#[test]
fn text_basic_results() {
    run_with_faked_timers(async {
        let fx = setup();
        let provided_results = vec![make_text_result(&root_folder_a(), "file1.ts"), make_text_result(&root_folder_a(), "file2.ts")];

        fx.register_test_text_search_provider(reporting_text_provider(provided_results.clone()), "file").await;

        let run = fx.run_text_search(simple_text_query("foo")).await.expect("runTextSearch");
        assert!(!run.limit_hit());
        assert_results(&run.results, &provided_results);
        fx.teardown().await;
    });
}

#[test]
fn text_all_provider_calls_get_global_include_excludes() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_text_search_provider(
            text_provider(|_, options, _, _| {
                assert_eq!(options.common.includes.len(), 1);
                assert_eq!(options.common.excludes.len(), 1);
                Promise::resolved(None)
            }),
            "file",
        )
        .await;

        let query = text_query(
            "foo",
            ICommonQueryProps {
                include_pattern: Some(expr(vec![("*.ts", ON)])),
                exclude_pattern: Some(expr(vec![("*.js", ON)])),
                ..ICommonQueryProps::new(vec![folder_query(root_folder_a()), folder_query(root_folder_b())])
            },
        );

        fx.run_text_search(query).await.expect("runTextSearch");
        fx.teardown().await;
    });
}

#[test]
fn text_global_local_include_excludes_combined() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_text_search_provider(
            text_provider(|_, options, _, _| {
                if is_folder(&options.common.folder, &root_folder_a()) {
                    assert_eq!(sorted(options.common.includes.clone()), ["*.ts", "foo"]);
                    assert_eq!(sorted(options.common.excludes.clone()), ["*.js", "bar"]);
                } else {
                    assert_eq!(sorted(options.common.includes.clone()), ["*.ts"]);
                    assert_eq!(sorted(options.common.excludes.clone()), ["*.js"]);
                }

                Promise::resolved(None)
            }),
            "file",
        )
        .await;

        let query = text_query(
            "foo",
            ICommonQueryProps {
                include_pattern: Some(expr(vec![("*.ts", ON)])),
                exclude_pattern: Some(expr(vec![("*.js", ON)])),
                ..ICommonQueryProps::new(vec![
                    IFolderQuery {
                        include_pattern: Some(expr(vec![("foo", ON)])),
                        exclude_pattern: Some(folder_excludes(expr(vec![("bar", ON)]))),
                        ..folder_query(root_folder_a())
                    },
                    folder_query(root_folder_b()),
                ])
            },
        );

        fx.run_text_search(query).await.expect("runTextSearch");
        fx.teardown().await;
    });
}

#[test]
fn text_include_excludes_resolved_correctly() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_text_search_provider(
            text_provider(|_, options, _, _| {
                assert_eq!(sorted(options.common.includes.clone()), ["*.jsx", "*.ts"]);
                assert_eq!(sorted(options.common.excludes.clone()), Vec::<String>::new());

                Promise::resolved(None)
            }),
            "file",
        )
        .await;

        let query = text_query(
            "foo",
            ICommonQueryProps {
                include_pattern: Some(expr(vec![("*.ts", ON), ("*.jsx", OFF)])),
                exclude_pattern: Some(expr(vec![("*.js", ON), ("*.tsx", OFF)])),
                ..ICommonQueryProps::new(vec![IFolderQuery {
                    include_pattern: Some(expr(vec![("*.jsx", ON)])),
                    exclude_pattern: Some(folder_excludes(expr(vec![("*.js", OFF)]))),
                    ..folder_query(root_folder_a())
                }])
            },
        );

        fx.run_text_search(query).await.expect("runTextSearch");
        fx.teardown().await;
    });
}

#[test]
fn text_provider_fail() {
    run_with_faked_timers(async {
        let fx = setup();
        fx.register_test_text_search_provider(text_provider(|_, _, _, _| errors::throw(Error::new("Provider fail"))), "file").await;

        // O `assert(false, 'Expected to fail')` do original é engolido pelo `catch {}` do mesmo `try`: o caso passa com ou sem erro.
        let _ = fx.run_text_search(simple_text_query("foo")).await;
        fx.teardown().await;
    });
}

#[test]
fn text_basic_sibling_clause() {
    run_with_faked_timers(async {
        let fx = setup();
        let root_a_path = fs_path(&root_folder_a());
        fx.ext_host_search.set_readdir(Rc::new(move |path: &str| {
            if path == root_a_path {
                Promise::resolved(vec!["file1.js".to_owned(), "file1.ts".to_owned()])
            } else {
                Promise::rejected(Error::new("Wrong path"))
            }
        }));

        let provided_results = vec![make_text_result(&root_folder_a(), "file1.js"), make_text_result(&root_folder_a(), "file1.ts")];

        fx.register_test_text_search_provider(reporting_text_provider(provided_results.clone()), "file").await;

        let query = text_query(
            "foo",
            ICommonQueryProps { exclude_pattern: Some(expr(vec![("*.js", when("$(basename).ts"))])), ..ICommonQueryProps::new(vec![folder_query(root_folder_a())]) },
        );

        let run = fx.run_text_search(query).await.expect("runTextSearch");
        assert_results(&run.results, &provided_results[1..]);
        fx.teardown().await;
    });
}

#[test]
fn text_multiroot_sibling_clause() {
    run_with_faked_timers(async {
        let fx = setup();
        let (folder_path, root_b_path) = (fs_path(&join_path(&root_folder_a(), &["folder"])), fs_path(&root_folder_b()));
        fx.ext_host_search.set_readdir(Rc::new(move |path: &str| {
            if path == folder_path {
                Promise::resolved(vec!["fileA.scss".to_owned(), "fileA.css".to_owned(), "file2.css".to_owned()])
            } else if path == root_b_path {
                Promise::resolved(vec!["fileB.ts".to_owned(), "fileB.js".to_owned(), "file3.js".to_owned()])
            } else {
                Promise::rejected(Error::new("Wrong path"))
            }
        }));

        fx.register_test_text_search_provider(
            text_provider(|_, options, progress, _| {
                let reported_results = if fs_path(&options.common.folder) == fs_path(&root_folder_a()) {
                    vec![
                        make_text_result(&root_folder_a(), "folder/fileA.scss"),
                        make_text_result(&root_folder_a(), "folder/fileA.css"),
                        make_text_result(&root_folder_a(), "folder/file2.css"),
                    ]
                } else {
                    vec![make_text_result(&root_folder_b(), "fileB.ts"), make_text_result(&root_folder_b(), "fileB.js"), make_text_result(&root_folder_b(), "file3.js")]
                };

                reported_results.into_iter().for_each(|result| progress.report(result));
                Promise::resolved(None)
            }),
            "file",
        )
        .await;

        let query = text_query(
            "foo",
            ICommonQueryProps {
                exclude_pattern: Some(expr(vec![("*.js", when("$(basename).ts")), ("*.css", ON)])),
                ..ICommonQueryProps::new(vec![
                    IFolderQuery { exclude_pattern: Some(folder_excludes(expr(vec![("folder/*.css", when("$(basename).scss"))]))), ..folder_query(root_folder_a()) },
                    IFolderQuery { exclude_pattern: Some(folder_excludes(expr(vec![("*.js", OFF)]))), ..folder_query(root_folder_b()) },
                ])
            },
        );

        let run = fx.run_text_search(query).await.expect("runTextSearch");
        assert_results(
            &run.results,
            &[
                make_text_result(&root_folder_a(), "folder/fileA.scss"),
                make_text_result(&root_folder_a(), "folder/file2.css"),
                make_text_result(&root_folder_b(), "fileB.ts"),
                make_text_result(&root_folder_b(), "fileB.js"),
                make_text_result(&root_folder_b(), "file3.js"),
            ],
        );
        fx.teardown().await;
    });
}

#[test]
fn text_include_pattern_applied() {
    run_with_faked_timers(async {
        let fx = setup();
        let provided_results = vec![make_text_result(&root_folder_a(), "file1.js"), make_text_result(&root_folder_a(), "file1.ts")];

        fx.register_test_text_search_provider(reporting_text_provider(provided_results.clone()), "file").await;

        let query = text_query("foo", ICommonQueryProps { include_pattern: Some(expr(vec![("*.ts", ON)])), ..ICommonQueryProps::new(vec![folder_query(root_folder_a())]) });

        let run = fx.run_text_search(query).await.expect("runTextSearch");
        assert_results(&run.results, &provided_results[1..]);
        fx.teardown().await;
    });
}

/// O corpo de `max results = N` e de `provider returns maxResults exactly` da subsuíte `Text:`: o provedor reporta `provided`, a
/// consulta pede `max_results`, e o caso confere o `limitHit`, os resultados e se o provedor foi cancelado.
async fn text_max_results_case(provided: Vec<TextSearchResult>, max_results: i64, expect_limit_hit: bool, expected_count: usize, expect_canceled: bool) {
    let fx = setup();
    let was_canceled = Rc::new(Cell::new(false));
    let (canceled, store, reported) = (was_canceled.clone(), fx.store.clone(), provided.clone());
    fx.register_test_text_search_provider(
        text_provider(move |_, _, progress, token| {
            let canceled = canceled.clone();
            store.add(token.on_cancellation_requested().listen(move |_| canceled.set(true)));
            reported.iter().for_each(|result| progress.report(result.clone()));
            Promise::resolved(None)
        }),
        "file",
    )
    .await;

    let query = text_query("foo", ICommonQueryProps { max_results: Some(max_results), ..ICommonQueryProps::new(vec![folder_query(root_folder_a())]) });

    let run = fx.run_text_search(query).await.expect("runTextSearch");
    assert_eq!(run.limit_hit(), expect_limit_hit, "limitHit");
    assert_results(&run.results, &provided[..expected_count]);
    assert_eq!(was_canceled.get(), expect_canceled, "canceled");
    fx.teardown().await;
}

#[test]
fn text_max_results_1() {
    let provided_results = vec![make_text_result(&root_folder_a(), "file1.ts"), make_text_result(&root_folder_a(), "file2.ts")];
    // Expected to return limitHit; Expected to be canceled
    run_with_faked_timers(text_max_results_case(provided_results, 1, true, 1, true));
}

#[test]
fn text_max_results_2() {
    let provided_results = vec![make_text_result(&root_folder_a(), "file1.ts"), make_text_result(&root_folder_a(), "file2.ts"), make_text_result(&root_folder_a(), "file3.ts")];
    // Expected to return limitHit; Expected to be canceled
    run_with_faked_timers(text_max_results_case(provided_results, 2, true, 2, true));
}

#[test]
fn text_provider_returns_max_results_exactly() {
    let provided_results = vec![make_text_result(&root_folder_a(), "file1.ts"), make_text_result(&root_folder_a(), "file2.ts")];
    // Expected not to return limitHit; Expected not to be canceled
    run_with_faked_timers(text_max_results_case(provided_results, 2, false, 2, false));
}

#[test]
fn text_provider_returns_early_with_limit_hit() {
    run_with_faked_timers(async {
        let fx = setup();
        let provided_results = vec![make_text_result(&root_folder_a(), "file1.ts"), make_text_result(&root_folder_a(), "file2.ts"), make_text_result(&root_folder_a(), "file3.ts")];

        let provided = provided_results.clone();
        fx.register_test_text_search_provider(
            text_provider(move |_, _, progress, _| {
                provided.iter().for_each(|result| progress.report(result.clone()));
                Promise::resolved(Some(TextSearchComplete { limit_hit: Some(true), message: None }))
            }),
            "file",
        )
        .await;

        let query = text_query("foo", ICommonQueryProps { max_results: Some(1000), ..ICommonQueryProps::new(vec![folder_query(root_folder_a())]) });

        let run = fx.run_text_search(query).await.expect("runTextSearch");
        assert!(run.limit_hit(), "Expected to return limitHit");
        assert_results(&run.results, &provided_results);
        fx.teardown().await;
    });
}

#[test]
fn text_multiroot_max_results() {
    run_with_faked_timers(async {
        let fx = setup();
        let cancels = Rc::new(Cell::new(0));
        let (counter, store) = (cancels.clone(), fx.store.clone());
        fx.register_test_text_search_provider(
            text_provider(move |_, options, progress, token| {
                let counter = counter.clone();
                store.add(token.on_cancellation_requested().listen(move |_| counter.set(counter.get() + 1)));
                let folder = options.common.folder.clone();
                async_fn(async move {
                    next_tick().await?;
                    ["file1.ts", "file2.ts", "file3.ts"].into_iter().for_each(|file| progress.report(make_text_result(&folder, file)));
                    Ok(None)
                })
            }),
            "file",
        )
        .await;

        let query = text_query("foo", ICommonQueryProps { max_results: Some(2), ..ICommonQueryProps::new(vec![folder_query(root_folder_a()), folder_query(root_folder_b())]) });

        let run = fx.run_text_search(query).await.expect("runTextSearch");
        assert_eq!(run.results.len(), 2);
        assert_eq!(cancels.get(), 2);
        fx.teardown().await;
    });
}

#[test]
fn text_works_with_non_file_schemes() {
    run_with_faked_timers(async {
        let fx = setup();
        let provided_results = vec![
            make_text_result(&fancy_scheme_folder_a(), "file1.ts"),
            make_text_result(&fancy_scheme_folder_a(), "file2.ts"),
            make_text_result(&fancy_scheme_folder_a(), "file3.ts"),
        ];

        fx.register_test_text_search_provider(reporting_text_provider(provided_results.clone()), FANCY_SCHEME).await;

        let query = text_query("foo", ICommonQueryProps::new(vec![folder_query(fancy_scheme_folder_a())]));

        let run = fx.run_text_search(query).await.expect("runTextSearch");
        assert_results(&run.results, &provided_results);
        fx.teardown().await;
    });
}
